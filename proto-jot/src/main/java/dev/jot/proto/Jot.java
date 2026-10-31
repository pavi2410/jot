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

/** A deliberately small, self-hosting Java build-tool prototype. */
public final class Jot {
    private Jot() {}

    public static void main(String[] args) throws Exception {
        String command = args.length == 0 ? "build" : args[0];
        Path root = Path.of("").toAbsolutePath().normalize();
        Config config = Config.read(root.resolve("jot.toml"));
        switch (command) {
            case "build" -> build(root, config);
            case "clean" -> {
                deleteTree(root.resolve("target"));
                System.out.println("clean: removed target");
            }
            default -> throw new IllegalArgumentException("usage: jot [build|clean]");
        }
    }

    private static void build(Path root, Config config) throws Exception {
        Path sourceRoot = root.resolve(config.layout.equals("maven")
                ? "src/main/java" : "src");
        List<Path> sources;
        try (Stream<Path> paths = Files.walk(sourceRoot)) {
            sources = paths.filter(path -> path.toString().endsWith(".java"))
                    .sorted().toList();
        }
        if (sources.isEmpty()) {
            throw new IllegalStateException("no Java sources under " + sourceRoot);
        }

        Path classes = root.resolve("target/classes");
        Files.createDirectories(classes);
        List<String> javac = new ArrayList<>(List.of(jdkTool("javac"), "--release",
                config.javaVersion, "-d", classes.toString()));
        sources.forEach(path -> javac.add(path.toString()));
        run(root, javac);

        Path artifact = root.resolve("target/" + config.name + "-" + config.version + ".jar");
        Files.createDirectories(artifact.getParent());
        List<String> jar = new ArrayList<>(List.of(jdkTool("jar"), "--create", "--file",
                artifact.toString()));
        if (!config.mainClass.isBlank()) {
            jar.add("--main-class");
            jar.add(config.mainClass);
        }
        jar.add("-C");
        jar.add(classes.toString());
        jar.add(".");
        run(root, jar);
        System.out.println("build: " + config.name + " " + config.version + " -> " + artifact);
    }

    private static void run(Path directory, List<String> command)
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
            for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) {
                Files.delete(path);
            }
        }
    }

    private record Config(String name, String version, String mainClass,
                          String layout, String javaVersion) {
        static Config read(Path file) throws IOException {
            Map<String, String> values = new TreeMap<>();
            String section = "";
            for (String raw : Files.readAllLines(file)) {
                String line = raw.split("#", 2)[0].trim();
                if (line.isEmpty()) continue;
                if (line.startsWith("[") && line.endsWith("]")) {
                    section = line.substring(1, line.length() - 1).trim();
                    continue;
                }
                int equals = line.indexOf('=');
                if (equals < 1) throw new IllegalArgumentException("invalid config line: " + raw);
                String key = line.substring(0, equals).trim();
                String value = line.substring(equals + 1).trim();
                if (value.length() < 2 || value.charAt(0) != '"'
                        || value.charAt(value.length() - 1) != '"') {
                    throw new IllegalArgumentException("expected quoted string: " + raw);
                }
                values.put(section + "." + key,
                        value.substring(1, value.length() - 1));
            }
            return new Config(required(values, "project.name"),
                    values.getOrDefault("project.version", "0.1.0"),
                    values.getOrDefault("project.main-class", ""),
                    values.getOrDefault("project.layout", "flat"),
                    required(values, "toolchains.java"));
        }

        private static String required(Map<String, String> values, String key) {
            String value = values.get(key);
            if (value == null || value.isBlank()) {
                throw new IllegalArgumentException("missing " + key + " in jot.toml");
            }
            return value;
        }
    }
}
