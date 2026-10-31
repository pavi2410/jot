package dev.jot.proto;

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
import java.util.Map;
import java.util.Properties;
import java.util.TreeMap;
import java.util.stream.Stream;

/** A small, self-hosting Java implementation of Jot's build lifecycle. */
public final class Jot {
    private static final String VERSION = "0.2.0-dev";

    private Jot() {}

    public static void main(String[] args) {
        try {
            execute(args);
        } catch (Exception error) {
            System.err.println("error: " + error.getMessage());
            System.exit(1);
        }
    }

    private static void execute(String[] args) throws Exception {
        List<String> arguments = new ArrayList<>(List.of(args));
        if (arguments.isEmpty() || arguments.getFirst().equals("--help")
                || arguments.getFirst().equals("-h")) {
            help();
            return;
        }
        if (arguments.getFirst().equals("--version") || arguments.getFirst().equals("-V")) {
            System.out.println("jot " + VERSION);
            return;
        }

        String command = arguments.removeFirst();
        Path projectRoot = null;
        String selectedModule = null;
        List<String> runArgs = new ArrayList<>();
        boolean programArgs = false;
        for (int i = 0; i < arguments.size(); i++) {
            String argument = arguments.get(i);
            if (programArgs) {
                runArgs.add(argument);
            } else if (argument.equals("--")) {
                programArgs = true;
            } else if (argument.equals("--project")) {
                if (++i == arguments.size()) throw new IllegalArgumentException("--project needs a path");
                projectRoot = Path.of(arguments.get(i)).toAbsolutePath().normalize();
            } else if (argument.equals("--module")) {
                if (++i == arguments.size()) throw new IllegalArgumentException("--module needs a name");
                selectedModule = arguments.get(i);
            } else {
                throw new IllegalArgumentException("unknown argument: " + argument);
            }
        }

        if (projectRoot == null) projectRoot = findProjectRoot(Path.of("").toAbsolutePath());
        Config config = Config.read(projectRoot.resolve("jot.toml"), null);
        switch (command) {
            case "build" -> {
                if (config.workspaceMembers.isEmpty()) build(projectRoot, config, List.of());
                else buildWorkspace(projectRoot, config, selectedModule);
            }
            case "run" -> {
                if (config.workspaceMembers.isEmpty()) {
                    if (selectedModule != null) throw new IllegalArgumentException("--module requires a workspace");
                    if (config.mainClass.isBlank()) {
                        throw new IllegalArgumentException("project.main-class is required for `jot run`");
                    }
                    build(projectRoot, config, List.of());
                    run(projectRoot, config, List.of(), runArgs);
                } else {
                    Workspace workspace = Workspace.load(projectRoot, config);
                    Project selected = workspace.selectRunnable(selectedModule);
                    buildWorkspace(projectRoot, config, selected.name());
                    run(selected.root, selected.config, selected.dependencies(), runArgs);
                }
            }
            case "clean" -> {
                if (config.workspaceMembers.isEmpty()) {
                    deleteTree(projectRoot.resolve("target"));
                    System.out.println("clean: " + config.name);
                } else {
                    Workspace workspace = Workspace.load(projectRoot, config);
                    for (Project project : workspace.ordered) {
                        deleteTree(project.root.resolve("target"));
                        System.out.println("clean: " + project.config.name);
                    }
                }
            }
            default -> throw new IllegalArgumentException("unknown command: " + command);
        }
    }

    private static void help() {
        System.out.println("jot " + VERSION + " — a Java build tool");
        System.out.println("Usage: jot <build|run|clean> [--project <directory>] [--module <name>] [-- <args...>]");
    }

    private static Path findProjectRoot(Path start) {
        for (Path current = start; current != null; current = current.getParent()) {
            if (Files.isRegularFile(current.resolve("jot.toml"))) return current;
        }
        throw new IllegalArgumentException("could not find jot.toml from " + start);
    }

    private static void build(Path root, Config config, List<Project> dependencies) throws Exception {
        List<Path> sources = new ArrayList<>();
        for (String sourceDirectory : config.sourceDirectories) {
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
                    + config.sourceDirectories);
        }

        Path classes = root.resolve(config.outputDirectory).normalize();
        Path target = root.resolve("target");
        if (!classes.startsWith(target) || classes.equals(target)) {
            throw new IllegalArgumentException("build.output-directory must be below target/");
        }
        Path artifact = root.resolve("target/" + config.name + "-" + config.version + ".jar");
        Path stateFile = root.resolve("target/.jot/build-state.properties");
        String fingerprint = fingerprint(root, sources, config, dependencies);
        if (Files.isRegularFile(artifact) && fingerprint.equals(readFingerprint(stateFile))) {
            System.out.println("build: " + config.name + " is up to date");
            return;
        }

        deleteTree(classes);
        Files.createDirectories(classes);
        List<String> javac = new ArrayList<>(List.of(jdkTool("javac"), "--release",
                config.javaVersion, "-d", classes.toString()));
        String classpath = classpath(dependencies);
        if (!classpath.isBlank()) javac.addAll(List.of("-classpath", classpath));
        sources.forEach(path -> javac.add(path.toString()));
        runProcess(root, javac);

        Files.createDirectories(artifact.getParent());
        List<String> jar = new ArrayList<>(List.of(jdkTool("jar"), "--create", "--file",
                artifact.toString(), "--date=1980-01-01T00:00:02Z"));
        if (!config.mainClass.isBlank()) jar.addAll(List.of("--main-class", config.mainClass));
        jar.addAll(List.of("-C", classes.toString(), "."));
        runProcess(root, jar);
        Properties state = new Properties();
        state.setProperty("fingerprint", fingerprint);
        Files.createDirectories(stateFile.getParent());
        try (var output = Files.newOutputStream(stateFile)) {
            state.store(output, "Jot content fingerprint. Generated file.");
        }
        System.out.println("build: " + config.name + " " + config.version + " -> " + artifact);
    }

    private static String fingerprint(Path root, List<Path> sources, Config config,
                                      List<Project> dependencies)
            throws IOException, NoSuchAlgorithmException {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        updateDigest(digest, config.javaVersion);
        updateDigest(digest, config.mainClass);
        updateDigest(digest, config.sourceDirectories.toString());
        updateDigest(digest, config.outputDirectory);
        updateDigest(digest, System.getProperty("java.version"));
        for (Path source : sources) {
            updateDigest(digest, root.relativize(source).toString());
            digest.update(Files.readAllBytes(source));
        }
        for (Project dependency : dependencies) {
            updateDigest(digest, dependency.root.toString());
            Path dependencyClasses = dependency.root.resolve(dependency.config.outputDirectory);
            if (Files.isDirectory(dependencyClasses)) {
                try (Stream<Path> files = Files.walk(dependencyClasses)) {
                    for (Path file : files.filter(Files::isRegularFile).sorted().toList()) {
                        updateDigest(digest, dependencyClasses.relativize(file).toString());
                        digest.update(Files.readAllBytes(file));
                    }
                }
            }
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

    private static void run(Path root, Config config, List<Project> dependencies,
                            List<String> arguments)
            throws IOException, InterruptedException {
        List<String> classpathEntries = new ArrayList<>();
        classpathEntries.add(root.resolve(config.outputDirectory).toString());
        for (Project dependency : dependencies) {
            classpathEntries.add(dependency.root.resolve(dependency.config.outputDirectory).toString());
        }
        List<String> command = new ArrayList<>(List.of(jdkTool("java"), "-cp",
                String.join(System.getProperty("path.separator"), classpathEntries), config.mainClass));
        command.addAll(arguments);
        runProcess(root, command);
    }

    private static String classpath(List<Project> dependencies) {
        return String.join(System.getProperty("path.separator"), dependencies.stream()
                .map(project -> project.root.resolve(project.config.outputDirectory).toString())
                .toList());
    }

    private static void buildWorkspace(Path root, Config rootConfig, String selectedModule)
            throws Exception {
        Workspace workspace = Workspace.load(root, rootConfig);
        List<Project> buildOrder = workspace.ordered;
        if (selectedModule != null) {
            Project selected = workspace.find(selectedModule);
            buildOrder = workspace.dependenciesFirst(selected);
        }
        for (Project project : buildOrder) {
            build(project.root, project.config, project.dependencies);
        }
    }

    private record Project(Path root, Config config, List<Project> dependencies) {
        String name() { return config.name; }
    }

    private record Workspace(List<Project> ordered, Map<Path, Project> byPath) {
        static Workspace load(Path root, Config rootConfig) throws IOException {
            List<Project> ordered = new ArrayList<>();
            Map<Path, Project> byPath = new TreeMap<>();
            List<Path> visiting = new ArrayList<>();
            for (String member : rootConfig.workspaceMembers) {
                visit(root.resolve(member), rootConfig.javaVersion, visiting, byPath, ordered);
            }
            return new Workspace(List.copyOf(ordered), Map.copyOf(byPath));
        }

        private static Project visit(Path projectRoot, String inheritedJava, List<Path> visiting,
                                     Map<Path, Project> byPath, List<Project> ordered)
                throws IOException {
            Path normalizedRoot = projectRoot.toAbsolutePath().normalize();
            Project known = byPath.get(normalizedRoot);
            if (known != null) return known;
            if (visiting.contains(normalizedRoot)) {
                List<String> cycle = new ArrayList<>();
                for (Path path : visiting) cycle.add(path.getFileName().toString());
                cycle.add(normalizedRoot.getFileName().toString());
                throw new IllegalArgumentException("workspace path dependency cycle: "
                        + String.join(" -> ", cycle));
            }
            Path manifest = normalizedRoot.resolve("jot.toml");
            if (!Files.isRegularFile(manifest)) {
                throw new IllegalArgumentException("missing project manifest: " + manifest);
            }
            Config config = Config.read(manifest, inheritedJava);
            visiting.add(normalizedRoot);
            List<Project> dependencies = new ArrayList<>();
            for (String dependencyPath : config.pathDependencies.values()) {
                dependencies.add(visit(normalizedRoot.resolve(dependencyPath), config.javaVersion,
                        visiting, byPath, ordered));
            }
            visiting.removeLast();
            Project project = new Project(normalizedRoot, config, List.copyOf(dependencies));
            byPath.put(normalizedRoot, project);
            ordered.add(project);
            return project;
        }

        Project find(String name) {
            return ordered.stream().filter(project -> project.name().equals(name)
                    || project.root.getFileName().toString().equals(name)).findFirst()
                    .orElseThrow(() -> new IllegalArgumentException("unknown workspace module `"
                            + name + "`"));
        }

        Project selectRunnable(String selectedModule) {
            if (selectedModule != null) {
                Project selected = find(selectedModule);
                if (selected.config.mainClass.isBlank()) {
                    throw new IllegalArgumentException("workspace module `" + selectedModule
                            + "` has no project.main-class");
                }
                return selected;
            }
            List<Project> runnable = ordered.stream()
                    .filter(project -> !project.config.mainClass.isBlank()).toList();
            if (runnable.size() != 1) {
                throw new IllegalArgumentException("workspace `jot run` needs --module when it has "
                        + runnable.size() + " runnable modules");
            }
            return runnable.getFirst();
        }

        List<Project> dependenciesFirst(Project project) {
            List<Project> result = new ArrayList<>();
            appendDependenciesFirst(project, new java.util.HashSet<>(), result);
            return result;
        }

        private static void appendDependenciesFirst(Project project, java.util.Set<Path> seen,
                                                    List<Project> result) {
            if (!seen.add(project.root)) return;
            for (Project dependency : project.dependencies) {
                appendDependenciesFirst(dependency, seen, result);
            }
            result.add(project);
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

    private record Config(String name, String version, String mainClass, String javaVersion,
                          List<String> sourceDirectories, String outputDirectory,
                          List<String> workspaceMembers, Map<String, String> pathDependencies) {
        static Config read(Path file, String inheritedJava) throws IOException {
            Map<String, Object> values = new TreeMap<>();
            String section = "";
            int lineNumber = 0;
            for (String raw : Files.readAllLines(file)) {
                lineNumber++;
                String line = stripComment(raw).trim();
                if (line.isEmpty()) continue;
                if (line.startsWith("[") && line.endsWith("]")) {
                    section = line.substring(1, line.length() - 1).trim();
                    continue;
                }
                int equals = line.indexOf('=');
                if (equals < 1) throw syntax(file, lineNumber, "expected key = value");
                String key = section + "." + line.substring(0, equals).trim();
                if (values.putIfAbsent(key, parseValue(line.substring(equals + 1).trim(), file,
                        lineNumber)) != null) {
                    throw syntax(file, lineNumber, "duplicate key " + key);
                }
            }
            String projectName = stringValue(values, "project.name", "", file);
            if (projectName.isBlank()) {
                Path parent = file.toAbsolutePath().getParent();
                projectName = parent == null ? "workspace" : parent.getFileName().toString();
            }
            String javaVersion = stringValue(values, "toolchains.java", inheritedJava, file);
            if (javaVersion == null || javaVersion.isBlank()) {
                throw new IllegalArgumentException("missing toolchains.java in " + file);
            }
            Map<String, String> pathDependencies = new TreeMap<>();
            for (Map.Entry<String, Object> entry : values.entrySet()) {
                if (!entry.getKey().startsWith("dependencies.")) continue;
                String dependencyName = entry.getKey().substring("dependencies.".length());
                if (!(entry.getValue() instanceof Map<?, ?> dependency)) {
                    throw new IllegalArgumentException("external dependency `" + dependencyName
                            + "` is not supported by this prototype yet");
                }
                Object path = dependency.get("path");
                if (!(path instanceof String pathString) || pathString.isBlank()) {
                    throw new IllegalArgumentException("dependency `" + dependencyName
                            + "` requires a non-empty path");
                }
                pathDependencies.put(dependencyName, pathString);
            }
            return new Config(projectName,
                    stringValue(values, "project.version", "0.1.0", file),
                    stringValue(values, "project.main-class", "", file),
                    javaVersion,
                    stringList(values, "build.source-dirs", List.of("src"), file),
                    stringValue(values, "build.output-directory", "target/classes", file),
                    stringList(values, "workspace.members", List.of(), file),
                    Map.copyOf(pathDependencies));
        }

        private static Object parseValue(String input, Path file, int lineNumber) {
            if (input.startsWith("\"") && input.endsWith("\"") && input.length() >= 2) {
                return input.substring(1, input.length() - 1);
            }
            if (input.startsWith("[") && input.endsWith("]")) {
                String contents = input.substring(1, input.length() - 1).trim();
                if (contents.isEmpty()) return List.of();
                List<String> result = new ArrayList<>();
                for (String item : contents.split(",")) {
                    String value = item.trim();
                    if (value.length() < 2 || !value.startsWith("\"") || !value.endsWith("\"")) {
                        throw syntax(file, lineNumber, "array values must be quoted strings");
                    }
                    result.add(value.substring(1, value.length() - 1));
                }
                return List.copyOf(result);
            }
            if (input.startsWith("{") && input.endsWith("}")) {
                String contents = input.substring(1, input.length() - 1).trim();
                Map<String, String> result = new TreeMap<>();
                if (contents.isEmpty()) return Map.copyOf(result);
                for (String item : splitInlineTable(contents)) {
                    int equals = item.indexOf('=');
                    if (equals < 1) throw syntax(file, lineNumber, "invalid inline table entry");
                    String key = item.substring(0, equals).trim();
                    String rawValue = item.substring(equals + 1).trim();
                    if (rawValue.length() < 2 || !rawValue.startsWith("\"")
                            || !rawValue.endsWith("\"")) {
                        throw syntax(file, lineNumber, "inline table values must be quoted strings");
                    }
                    result.put(key, rawValue.substring(1, rawValue.length() - 1));
                }
                return Map.copyOf(result);
            }
            throw syntax(file, lineNumber,
                    "only quoted strings, string arrays, and inline tables are supported");
        }

        private static List<String> splitInlineTable(String contents) {
            List<String> entries = new ArrayList<>();
            boolean quoted = false;
            int start = 0;
            for (int i = 0; i < contents.length(); i++) {
                char character = contents.charAt(i);
                if (character == '"') quoted = !quoted;
                else if (character == ',' && !quoted) {
                    entries.add(contents.substring(start, i).trim());
                    start = i + 1;
                }
            }
            entries.add(contents.substring(start).trim());
            return entries;
        }

        private static String stripComment(String value) {
            boolean quoted = false;
            boolean escaped = false;
            for (int i = 0; i < value.length(); i++) {
                char character = value.charAt(i);
                if (escaped) escaped = false;
                else if (character == '\\' && quoted) escaped = true;
                else if (character == '"') quoted = !quoted;
                else if (character == '#' && !quoted) return value.substring(0, i);
            }
            return value;
        }

        private static String stringValue(Map<String, Object> values, String key, String fallback,
                                          Path file) {
            Object value = values.get(key);
            if (value == null) return fallback;
            if (value instanceof String string) return string;
            throw new IllegalArgumentException(key + " must be a string in " + file);
        }

        @SuppressWarnings("unchecked")
        private static List<String> stringList(Map<String, Object> values, String key,
                                               List<String> fallback, Path file) {
            Object value = values.get(key);
            if (value == null) return fallback;
            if (value instanceof List<?>) return (List<String>) value;
            throw new IllegalArgumentException(key + " must be an array of strings in " + file);
        }

        private static IllegalArgumentException syntax(Path file, int lineNumber, String message) {
            return new IllegalArgumentException(file + ":" + lineNumber + ": " + message);
        }
    }
}
