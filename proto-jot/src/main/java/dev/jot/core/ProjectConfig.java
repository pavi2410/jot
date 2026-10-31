package dev.jot.core;

import dev.jot.core.config.TomlDocument;
import java.io.IOException;
import java.nio.file.Path;
import java.util.Map;
import java.util.TreeMap;

/** The explicit build inputs read from a project's jot.toml. */
public record ProjectConfig(String name, String version, String mainClass, String javaVersion,
                            java.util.List<String> sourceDirectories, String outputDirectory,
                            java.util.List<String> workspaceMembers,
                            Map<String, String> pathDependencies) {
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
        Map<String, String> pathDependencies = new TreeMap<>();
        for (Map.Entry<String, Object> entry : document.values().entrySet()) {
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
        return new ProjectConfig(projectName,
                document.string("project.version", "0.1.0"),
                document.string("project.main-class", ""), javaVersion,
                document.stringList("build.source-dirs", java.util.List.of("src")),
                document.string("build.output-directory", "target/classes"),
                document.stringList("workspace.members", java.util.List.of()),
                java.util.Collections.unmodifiableMap(new TreeMap<>(pathDependencies)));
    }
}
