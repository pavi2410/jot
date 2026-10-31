package dev.jot.core;

import dev.jot.core.resolve.MavenResolver;
import dev.jot.core.resolve.Lockfile;
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
        List<ResourceFile> resources = collectResources(root, config.resourceDirectories());

        Path classes = root.resolve(config.outputDirectory()).normalize();
        Path target = root.resolve("target");
        if (!classes.startsWith(target) || classes.equals(target)) {
            throw new IllegalArgumentException("build.output-directory must be below target/");
        }
        Path artifact = target.resolve(config.name() + "-" + config.version() + ".jar");
        Path stateFile = target.resolve(".jot/build-state.properties");
        String fingerprint = fingerprint(project, sources, resources, externalArtifacts);
        if (outputsAreIntact(classes, artifact, stateFile, fingerprint)) {
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
        copyResources(resources, classes);

        Files.createDirectories(artifact.getParent());
        List<String> jar = new ArrayList<>(List.of(jdkTool("jar"), "--create", "--file",
                artifact.toString(), "--date=1980-01-01T00:00:02Z"));
        if (!config.mainClass().isBlank()) jar.addAll(List.of("--main-class", config.mainClass()));
        jar.addAll(List.of("-C", classes.toString(), "."));
        runProcess(root, jar);
        Properties state = new Properties();
        state.setProperty("fingerprint", fingerprint);
        state.setProperty("classes-sha256", directoryFingerprint(classes));
        state.setProperty("artifact-sha256", fileFingerprint(artifact));
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

    public static void lock(ProjectWorkspace workspace, Path lockPath, MavenResolver resolver)
            throws Exception {
        java.util.Map<String, String> declarations = new java.util.LinkedHashMap<>();
        int index = 0;
        for (WorkspaceProject project : workspace.ordered()) {
            for (String coordinate : project.config().dependencies().values()) {
                declarations.put("dependency-" + index++, coordinate);
            }
            for (String coordinate : project.config().testDependencies().values()) {
                declarations.put("test-dependency-" + index++, coordinate);
            }
        }
        List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(declarations);
        Lockfile.create(declarations.values(), resolved).write(lockPath);
        System.out.println("lock: wrote " + lockPath);
    }

    public static void test(WorkspaceProject project, MavenResolver resolver) throws Exception {
        build(project, resolver);
        Path root = project.root();
        ProjectConfig config = project.config();
        List<Path> testSources = collectSources(root, config.testSourceDirectories());
        if (testSources.isEmpty()) {
            System.out.println("test: no test sources in " + config.name());
            return;
        }
        Path testClasses = root.resolve(config.testOutputDirectory()).normalize();
        Path target = root.resolve("target");
        if (!testClasses.startsWith(target) || testClasses.equals(target)) {
            throw new IllegalArgumentException("test.output-directory must be below target/");
        }
        deleteTree(testClasses);
        Files.createDirectories(testClasses);

        List<MavenResolver.ResolvedArtifact> mainArtifacts = resolveExternalArtifacts(project, resolver);
        List<MavenResolver.ResolvedArtifact> testArtifacts = resolver.resolve(config.testDependencies());
        Path lockPath = findLockfile(root);
        if (lockPath != null) Lockfile.read(lockPath).verify(config.testDependencies().values(), testArtifacts);
        List<String> classpathEntries = new ArrayList<>();
        classpathEntries.add(root.resolve(config.outputDirectory()).toString());
        for (WorkspaceProject dependency : transitiveDependencies(project)) {
            classpathEntries.add(dependency.root().resolve(dependency.config().outputDirectory()).toString());
        }
        mainArtifacts.stream().map(artifact -> artifact.jar().toString()).forEach(classpathEntries::add);
        testArtifacts.stream().map(artifact -> artifact.jar().toString()).forEach(classpathEntries::add);
        List<String> testClasspath = List.copyOf(classpathEntries);

        List<String> javac = new ArrayList<>(List.of(jdkTool("javac"), "--release",
                config.javaVersion(), "-classpath",
                String.join(System.getProperty("path.separator"), testClasspath),
                "-d", testClasses.toString()));
        testSources.forEach(path -> javac.add(path.toString()));
        runProcess(root, javac);

        List<String> runtimeClasspath = new ArrayList<>();
        runtimeClasspath.add(testClasses.toString());
        runtimeClasspath.addAll(testClasspath);
        List<String> runner = new ArrayList<>(List.of(jdkTool("java"), "-cp",
                String.join(System.getProperty("path.separator"), runtimeClasspath),
                "--enable-final-field-mutation=ALL-UNNAMED",
                "org.junit.platform.console.ConsoleLauncher", "execute", "--scan-class-path",
                "--fail-if-no-tests", "--disable-ansi-colors"));
        runProcess(root, runner);
    }

    private static String fingerprint(WorkspaceProject project, List<Path> sources,
                                      List<ResourceFile> resources,
                                      List<MavenResolver.ResolvedArtifact> externalArtifacts)
            throws IOException, NoSuchAlgorithmException {
        Path root = project.root();
        ProjectConfig config = project.config();
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        updateDigest(digest, config.javaVersion());
        updateDigest(digest, config.mainClass());
        updateDigest(digest, config.sourceDirectories().toString());
        updateDigest(digest, config.resourceDirectories().toString());
        updateDigest(digest, config.outputDirectory());
        updateDigest(digest, System.getProperty("java.version"));
        for (Path source : sources) {
            updateDigest(digest, root.relativize(source).toString());
            digest.update(Files.readAllBytes(source));
        }
        for (ResourceFile resource : resources) {
            updateDigest(digest, resource.relativePath().toString());
            digest.update(Files.readAllBytes(resource.source()));
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

    private static boolean outputsAreIntact(Path classes, Path artifact, Path stateFile,
                                            String fingerprint) throws Exception {
        if (!Files.isDirectory(classes) || !Files.isRegularFile(artifact)
                || !Files.isRegularFile(stateFile)) return false;
        Properties state = new Properties();
        try (var input = Files.newInputStream(stateFile)) {
            state.load(input);
        }
        return fingerprint.equals(state.getProperty("fingerprint"))
                && directoryFingerprint(classes).equals(state.getProperty("classes-sha256"))
                && fileFingerprint(artifact).equals(state.getProperty("artifact-sha256"));
    }

    private static String directoryFingerprint(Path directory) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (Stream<Path> paths = Files.walk(directory)) {
            for (Path file : paths.filter(Files::isRegularFile).sorted().toList()) {
                updateDigest(digest, directory.relativize(file).toString());
                digest.update(Files.readAllBytes(file));
            }
        }
        return HexFormat.of().formatHex(digest.digest());
    }

    private static String fileFingerprint(Path file) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256")
                .digest(Files.readAllBytes(file)));
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

    private static List<Path> collectSources(Path root, List<String> sourceDirectories)
            throws IOException {
        List<Path> sources = new ArrayList<>();
        for (String sourceDirectory : sourceDirectories) {
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
        return sources;
    }

    private static List<ResourceFile> collectResources(Path root, List<String> resourceDirectories)
            throws IOException {
        java.util.Map<Path, ResourceFile> resources = new java.util.LinkedHashMap<>();
        for (String directory : resourceDirectories) {
            Path resourceRoot = root.resolve(directory).normalize();
            if (!resourceRoot.startsWith(root)) {
                throw new IllegalArgumentException("resource directory escapes project: " + directory);
            }
            if (!Files.isDirectory(resourceRoot)) continue;
            try (Stream<Path> paths = Files.walk(resourceRoot)) {
                for (Path file : paths.filter(Files::isRegularFile).toList()) {
                    Path relativePath = resourceRoot.relativize(file);
                    ResourceFile previous = resources.putIfAbsent(relativePath,
                            new ResourceFile(file, relativePath));
                    if (previous != null && !previous.source().equals(file)) {
                        throw new IllegalArgumentException("resource path is declared more than once: "
                                + relativePath);
                    }
                }
            }
        }
        return resources.values().stream()
                .sorted(Comparator.comparing(resource -> resource.relativePath().toString()))
                .toList();
    }

    private static void copyResources(List<ResourceFile> resources, Path classes)
            throws IOException {
        for (ResourceFile resource : resources) {
            Path destination = classes.resolve(resource.relativePath()).normalize();
            if (!destination.startsWith(classes)) {
                throw new IllegalArgumentException("resource path escapes build output: "
                        + resource.relativePath());
            }
            Files.createDirectories(destination.getParent());
            Files.copy(resource.source(), destination);
        }
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
        List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(declarations);
        Path lockPath = findLockfile(project.root());
        if (lockPath != null) Lockfile.read(lockPath).verify(declarations.values(), resolved);
        return resolved;
    }

    private static Path findLockfile(Path projectRoot) {
        for (Path current = projectRoot; current != null; current = current.getParent()) {
            Path candidate = current.resolve("jot.lock");
            if (Files.isRegularFile(candidate)) return candidate;
        }
        return null;
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

    private record ResourceFile(Path source, Path relativePath) {}
}
