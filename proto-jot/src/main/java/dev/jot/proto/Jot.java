package dev.jot.proto;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
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
            } else {
                throw new IllegalArgumentException("unknown argument: " + argument);
            }
        }

        if (projectRoot == null) projectRoot = findProjectRoot(Path.of("").toAbsolutePath());
        Config config = Config.read(projectRoot.resolve("jot.toml"));
        switch (command) {
            case "build" -> build(projectRoot, config);
            case "run" -> {
                if (config.mainClass.isBlank()) {
                    throw new IllegalArgumentException("project.main-class is required for `jot run`");
                }
                build(projectRoot, config);
                run(projectRoot, config, runArgs);
            }
            case "clean" -> {
                deleteTree(projectRoot.resolve(config.outputDirectory));
                deleteTree(projectRoot.resolve("target"));
                System.out.println("clean: " + config.name);
            }
            default -> throw new IllegalArgumentException("unknown command: " + command);
        }
    }

    private static void help() {
        System.out.println("jot " + VERSION + " — a Java build tool");
        System.out.println("Usage: jot <build|run|clean> [--project <directory>] [-- <args...>]");
    }

    private static Path findProjectRoot(Path start) {
        for (Path current = start; current != null; current = current.getParent()) {
            if (Files.isRegularFile(current.resolve("jot.toml"))) return current;
        }
        throw new IllegalArgumentException("could not find jot.toml from " + start);
    }

    private static void build(Path root, Config config) throws Exception {
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
        if (!classes.startsWith(root)) {
            throw new IllegalArgumentException("build.output-directory escapes project");
        }
        Files.createDirectories(classes);
        List<String> javac = new ArrayList<>(List.of(jdkTool("javac"), "--release",
                config.javaVersion, "-d", classes.toString()));
        sources.forEach(path -> javac.add(path.toString()));
        runProcess(root, javac);

        Path artifact = root.resolve("target/" + config.name + "-" + config.version + ".jar");
        Files.createDirectories(artifact.getParent());
        List<String> jar = new ArrayList<>(List.of(jdkTool("jar"), "--create", "--file",
                artifact.toString()));
        if (!config.mainClass.isBlank()) jar.addAll(List.of("--main-class", config.mainClass));
        jar.addAll(List.of("-C", classes.toString(), "."));
        runProcess(root, jar);
        System.out.println("build: " + config.name + " " + config.version + " -> " + artifact);
    }

    private static void run(Path root, Config config, List<String> arguments)
            throws IOException, InterruptedException {
        List<String> command = new ArrayList<>(List.of(jdkTool("java"), "-cp",
                root.resolve(config.outputDirectory).toString(), config.mainClass));
        command.addAll(arguments);
        runProcess(root, command);
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
                          List<String> sourceDirectories, String outputDirectory) {
        static Config read(Path file) throws IOException {
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
            return new Config(requiredString(values, "project.name", file),
                    stringValue(values, "project.version", "0.1.0", file),
                    stringValue(values, "project.main-class", "", file),
                    requiredString(values, "toolchains.java", file),
                    stringList(values, "build.source-dirs", List.of("src"), file),
                    stringValue(values, "build.output-directory", "target/classes", file));
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
            throw syntax(file, lineNumber, "only quoted strings and string arrays are supported");
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

        private static String requiredString(Map<String, Object> values, String key, Path file) {
            String value = stringValue(values, key, "", file);
            if (value.isBlank()) throw new IllegalArgumentException("missing " + key + " in " + file);
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
