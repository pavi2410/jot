package dev.jot.core;

import dev.jot.core.config.TomlDocument;
import java.io.IOException;
import java.nio.file.Path;
import java.util.Collections;
import java.util.Map;
import java.util.TreeMap;

/** The explicit build inputs read from a project's jot.toml. */
public record ProjectConfig(String name, String version, String mainClass, String javaVersion,
                            java.util.List<String> sourceDirectories, String outputDirectory,
                            java.util.List<String> resourceDirectories,
                            java.util.List<String> testSourceDirectories,
                            String testOutputDirectory,
                            java.util.List<String> workspaceMembers,
                            Map<String, String> pathDependencies,
                            Map<String, String> dependencies,
                            Map<String, String> testDependencies) {
    public boolean isWorkspace() {
        return !workspaceMembers.isEmpty();
    }

    public static ProjectConfig read(Path file, String inheritedJava) throws IOException {
        TomlDocument document = TomlDocument.read(file);
        String projectName = document.string("project.name", "");
        if (projectName.isBlank()) {
            Path parent = file.toAbsolutePath().getParent();
            projectName = parent == null ? "workspace" : parent.getFileName().toString();
        }
        String javaVersion = document.string("toolchains.java", inheritedJava);
        if (javaVersion == null || javaVersion.isBlank()) {
            throw new IllegalArgumentException("missing toolchains.java in " + file);
        }
        String layout = document.string("project.layout", "standard");
        if (!layout.equals("standard") && !layout.equals("maven")) {
            throw new IllegalArgumentException("project.layout must be `standard` or `maven` in " + file);
        }
        java.util.List<String> sourceDefaults = layout.equals("maven")
                ? java.util.List.of("src/main/java") : java.util.List.of("src");
        java.util.List<String> testSourceDefaults = layout.equals("maven")
                ? java.util.List.of("src/test/java") : java.util.List.of("test");
        java.util.List<String> resourceDefaults = layout.equals("maven")
                ? java.util.List.of("src/main/resources") : java.util.List.of("res");
        Map<String, String> pathDependencies = new TreeMap<>();
        Map<String, String> dependencies = parseDependencies(document.values(), "dependencies.",
                pathDependencies);
        Map<String, String> testDependencies = parseDependencies(document.values(),
                "test-dependencies.", new TreeMap<>());
        return new ProjectConfig(projectName,
                document.string("project.version", "0.1.0"),
                document.string("project.main-class", ""), javaVersion,
                document.stringList("build.source-dirs", sourceDefaults),
                document.string("build.output-directory", "target/classes"),
                document.stringList("build.resource-dirs", resourceDefaults),
                document.stringList("test.source-dirs", testSourceDefaults),
                document.string("test.output-directory", "target/test-classes"),
                document.stringList("workspace.members", java.util.List.of()),
                Collections.unmodifiableMap(new TreeMap<>(pathDependencies)),
                Collections.unmodifiableMap(new TreeMap<>(dependencies)),
                Collections.unmodifiableMap(new TreeMap<>(testDependencies)));
    }

    private static Map<String, String> parseDependencies(Map<String, Object> values, String prefix,
                                                         Map<String, String> pathDependencies) {
        Map<String, String> coordinates = new TreeMap<>();
        for (Map.Entry<String, Object> entry : values.entrySet()) {
            if (!entry.getKey().startsWith(prefix)) continue;
            String name = entry.getKey().substring(prefix.length());
            if (entry.getValue() instanceof String coordinate) {
                coordinates.put(name, coordinate);
                continue;
            }
            if (!(entry.getValue() instanceof Map<?, ?> specification)) {
                throw new IllegalArgumentException("invalid dependency declaration for `" + name + "`");
            }
            Object path = specification.get("path");
            Object coordinate = specification.get("coords");
            if (path instanceof String pathString && !pathString.isBlank()) {
                if (!prefix.equals("dependencies.")) {
                    throw new IllegalArgumentException("test dependency `" + name
                            + "` cannot be a path dependency");
                }
                pathDependencies.put(name, pathString);
            } else if (coordinate instanceof String coordinateString && !coordinateString.isBlank()) {
                coordinates.put(name, coordinateString);
            } else {
                throw new IllegalArgumentException("dependency `" + name
                        + "` requires `path` or `coords`");
            }
        }
        return coordinates;
    }
}
