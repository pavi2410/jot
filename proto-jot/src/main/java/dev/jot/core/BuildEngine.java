package dev.jot.core;

import dev.jot.core.resolve.MavenResolver;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.List;
import java.util.Properties;
import java.util.stream.Stream;

/** Compiles configured Java sources, packages JARs, and launches project mains. */
public final class BuildEngine {
    private BuildEngine() {}

    public static void build(WorkspaceProject project) throws Exception {
        build(project, new MavenResolver());
    }

    public static void build(WorkspaceProject project, MavenResolver resolver) throws Exception {
        Path root = project.root();
        ProjectConfig config = project.config();
        List<MavenResolver.ResolvedArtifact> externalArtifacts = resolveExternalArtifacts(project,
                resolver);
        List<Path> sources = new ArrayList<>();
        for (String sourceDirectory : config.sourceDirectories()) {
            Path sourceRoot = root.resolve(sourceDirectory).normalize();
            if (!sourceRoot.startsWith(root)) {
                throw new IllegalArgumentException("source directory escapes project: " + sourceDirectory);
            }
            if (!Files.isDirectory(sourceRoot)) continue;
            try (Stream<Path> paths = Files.walk(sourceRoot)) {
                sources.addAll(paths.filter(path -> path.toString().endsWith(".java")).toList());
            }
        }
        sources.sort(Comparator.naturalOrder());
        if (sources.isEmpty()) {
            throw new IllegalStateException("no Java sources in configured build.source-dirs: "
                    + config.sourceDirectories());
        }

        Path classes = root.resolve(config.outputDirectory()).normalize();
        Path target = root.resolve("target");
        if (!classes.startsWith(target) || classes.equals(target)) {
            throw new IllegalArgumentException("build.output-directory must be below target/");
        }
        Path artifact = target.resolve(config.name() + "-" + config.version() + ".jar");
        Path stateFile = target.resolve(".jot/build-state.properties");
        String fingerprint = fingerprint(project, sources, externalArtifacts);
        if (Files.isRegularFile(artifact) && fingerprint.equals(readFingerprint(stateFile))) {
            System.out.println("build: " + config.name() + " is up to date");
            return;
        }

        deleteTree(classes);
        Files.createDirectories(classes);
        List<String> javac = new ArrayList<>(List.of(jdkTool("javac"), "--release",
                config.javaVersion(), "-d", classes.toString()));
        String classpath = classpath(project, externalArtifacts);
        if (!classpath.isBlank()) javac.addAll(List.of("-classpath", classpath));
        sources.forEach(path -> javac.add(path.toString()));
        runProcess(root, javac);

        Files.createDirectories(artifact.getParent());
        List<String> jar = new ArrayList<>(List.of(jdkTool("jar"), "--create", "--file",
                artifact.toString(), "--date=1980-01-01T00:00:02Z"));
        if (!config.mainClass().isBlank()) jar.addAll(List.of("--main-class", config.mainClass()));
        jar.addAll(List.of("-C", classes.toString(), "."));
        runProcess(root, jar);
        Properties state = new Properties();
        state.setProperty("fingerprint", fingerprint);
        Files.createDirectories(stateFile.getParent());
        try (var output = Files.newOutputStream(stateFile)) {
            state.store(output, "Jot content fingerprint. Generated file.");
        }
        System.out.println("build: " + config.name() + " " + config.version() + " -> " + artifact);
    }

    public static void run(WorkspaceProject project, List<String> arguments)
            throws Exception {
        run(project, arguments, new MavenResolver());
    }

    public static void run(WorkspaceProject project, List<String> arguments,
                           MavenResolver resolver) throws Exception {
        List<String> classpathEntries = new ArrayList<>();
        classpathEntries.add(project.root().resolve(project.config().outputDirectory()).toString());
        for (WorkspaceProject dependency : transitiveDependencies(project)) {
            classpathEntries.add(dependency.root().resolve(dependency.config().outputDirectory()).toString());
        }
        for (MavenResolver.ResolvedArtifact artifact : resolveExternalArtifacts(project, resolver)) {
            classpathEntries.add(artifact.jar().toString());
        }
        List<String> command = new ArrayList<>(List.of(jdkTool("java"), "-cp",
                String.join(System.getProperty("path.separator"), classpathEntries),
                project.config().mainClass()));
        command.addAll(arguments);
        runProcess(project.root(), command);
    }

    public static void clean(WorkspaceProject project) throws IOException {
        deleteTree(project.root().resolve("target"));
        System.out.println("clean: " + project.config().name());
    }

    private static String fingerprint(WorkspaceProject project, List<Path> sources,
                                      List<MavenResolver.ResolvedArtifact> externalArtifacts)
            throws IOException, NoSuchAlgorithmException {
        Path root = project.root();
        ProjectConfig config = project.config();
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        updateDigest(digest, config.javaVersion());
        updateDigest(digest, config.mainClass());
        updateDigest(digest, config.sourceDirectories().toString());
        updateDigest(digest, config.outputDirectory());
        updateDigest(digest, System.getProperty("java.version"));
        for (Path source : sources) {
            updateDigest(digest, root.relativize(source).toString());
            digest.update(Files.readAllBytes(source));
        }
        for (WorkspaceProject dependency : transitiveDependencies(project)) {
            updateDigest(digest, dependency.root().toString());
            Path dependencyClasses = dependency.root().resolve(dependency.config().outputDirectory());
            if (Files.isDirectory(dependencyClasses)) {
                try (Stream<Path> files = Files.walk(dependencyClasses)) {
                    for (Path file : files.filter(Files::isRegularFile).sorted().toList()) {
                        updateDigest(digest, dependencyClasses.relativize(file).toString());
                        digest.update(Files.readAllBytes(file));
                    }
                }
            }
        }
        for (MavenResolver.ResolvedArtifact artifact : externalArtifacts) {
            updateDigest(digest, artifact.coordinate().notation());
            digest.update(Files.readAllBytes(artifact.jar()));
        }
        return HexFormat.of().formatHex(digest.digest());
    }

    private static void updateDigest(MessageDigest digest, String value) {
        digest.update(value.getBytes(StandardCharsets.UTF_8));
        digest.update((byte) 0);
    }

    private static String readFingerprint(Path file) throws IOException {
        if (!Files.isRegularFile(file)) return "";
        Properties state = new Properties();
        try (var input = Files.newInputStream(file)) {
            state.load(input);
        }
        return state.getProperty("fingerprint", "");
    }

    private static String classpath(WorkspaceProject project,
                                    List<MavenResolver.ResolvedArtifact> externalArtifacts) {
        List<String> entries = new ArrayList<>(transitiveDependencies(project).stream()
                .map(dependency -> dependency.root()
                        .resolve(dependency.config().outputDirectory()).toString())
                .toList());
        externalArtifacts.stream().map(artifact -> artifact.jar().toString()).forEach(entries::add);
        return String.join(System.getProperty("path.separator"), entries);
    }

    private static List<MavenResolver.ResolvedArtifact> resolveExternalArtifacts(
            WorkspaceProject project, MavenResolver resolver) throws Exception {
        List<WorkspaceProject> dependencyProjects = new ArrayList<>();
        dependencyProjects.add(project);
        dependencyProjects.addAll(transitiveDependencies(project));
        java.util.Map<String, String> declarations = new java.util.LinkedHashMap<>();
        int index = 0;
        for (WorkspaceProject dependencyProject : dependencyProjects) {
            for (String coordinate : dependencyProject.config().dependencies().values()) {
                declarations.put("dependency-" + index++, coordinate);
            }
        }
        return resolver.resolve(declarations);
    }

    private static List<WorkspaceProject> transitiveDependencies(WorkspaceProject project) {
        List<WorkspaceProject> result = new ArrayList<>();
        java.util.Set<Path> seen = new java.util.LinkedHashSet<>();
        for (WorkspaceProject dependency : project.dependencies()) {
            appendDependency(dependency, seen, result);
        }
        return result;
    }

    private static void appendDependency(WorkspaceProject project, java.util.Set<Path> seen,
                                         List<WorkspaceProject> result) {
        if (!seen.add(project.root())) return;
        result.add(project);
        for (WorkspaceProject dependency : project.dependencies()) {
            appendDependency(dependency, seen, result);
        }
    }

    private static void runProcess(Path directory, List<String> command)
            throws IOException, InterruptedException {
        int status = new ProcessBuilder(command).directory(directory.toFile())
                .inheritIO().start().waitFor();
        if (status != 0) {
            throw new IllegalStateException("command failed with exit code " + status + ": "
                    + String.join(" ", command));
        }
    }

    private static String jdkTool(String name) {
        String executable = System.getProperty("os.name").startsWith("Windows")
                ? name + ".exe" : name;
        return Path.of(System.getProperty("java.home"), "bin", executable).toString();
    }

    private static void deleteTree(Path root) throws IOException {
        if (!Files.exists(root)) return;
        try (Stream<Path> paths = Files.walk(root)) {
            for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
        }
    }
}
