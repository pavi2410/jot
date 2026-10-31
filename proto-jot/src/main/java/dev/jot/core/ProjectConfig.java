package dev.jot.core;

import dev.jot.core.config.TomlDocument;
import java.io.IOException;
import java.nio.file.Files;
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
        Map<String, Object> catalog = loadVersionCatalog(file.toAbsolutePath().getParent());
        Map<String, String> pathDependencies = new TreeMap<>();
        Map<String, String> dependencies = parseDependencies(document.table("dependencies"),
                pathDependencies, false, catalog, file);
        Map<String, String> testDependencies = parseDependencies(document.table("test-dependencies"),
                new TreeMap<>(), true, catalog, file);
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

    private static Map<String, String> parseDependencies(Map<String, Object> declarations,
                                                         Map<String, String> pathDependencies,
                                                         boolean testDependencies,
                                                         Map<String, Object> catalog, Path manifest)
            throws IOException {
        Map<String, String> coordinates = new TreeMap<>();
        for (Map.Entry<String, Object> entry : declarations.entrySet()) {
            String name = entry.getKey();
            if (entry.getValue() instanceof String coordinate) {
                coordinates.put(name, coordinate);
                continue;
            }
            if (!(entry.getValue() instanceof Map<?, ?> specification)) {
                throw new IllegalArgumentException("invalid dependency declaration for `" + name + "`");
            }
            Object coordinate = specification.get("coords");
            Object path = specification.get("path");
            Object catalogAlias = specification.get("catalog");
            if (coordinate instanceof String coordinateString && !coordinateString.isBlank()) {
                coordinates.put(name, coordinateString);
                continue;
            }
            if (path instanceof String pathString && !pathString.isBlank()) {
                if (testDependencies) {
                    throw new IllegalArgumentException("test dependency `" + name
                            + "` cannot be a path dependency");
                }
                pathDependencies.put(name, pathString);
            } else if (catalogAlias instanceof String alias && !alias.isBlank()) {
                coordinates.put(name, resolveCatalogDependency(name, alias, catalog, manifest));
            } else {
                throw new IllegalArgumentException("dependency `" + name
                        + "` requires `path` or `coords`");
            }
        }
        return coordinates;
    }

    private static Map<String, Object> loadVersionCatalog(Path start) throws IOException {
        for (Path directory = start; directory != null; directory = directory.getParent()) {
            Path catalogPath = directory.resolve("libs.versions.toml");
            if (Files.isRegularFile(catalogPath)) return TomlDocument.read(catalogPath).values();
        }
        return Map.of();
    }

    private static String resolveCatalogDependency(String dependency, String alias,
                                                   Map<String, Object> catalog, Path manifest)
            throws IOException {
        if (catalog.isEmpty()) {
            throw new IllegalArgumentException("dependency `" + dependency
                    + "` uses a catalog but no libs.versions.toml was found from " + manifest);
        }
        Object librariesValue = catalog.get("libraries");
        if (!(librariesValue instanceof Map<?, ?> libraries)
                || !(libraries.get(alias) instanceof Map<?, ?> library)) {
            throw new IllegalArgumentException("dependency `" + dependency
                    + "` references missing catalog entry `" + alias + "`");
        }
        Object moduleValue = library.get("module");
        if (!(moduleValue instanceof String module) || module.isBlank()) {
            throw new IllegalArgumentException("catalog entry `" + alias + "` needs a module in "
                    + manifest);
        }
        Object versionValue = library.get("version");
        String version = null;
        if (versionValue instanceof String literalVersion) {
            version = literalVersion;
        } else if (versionValue instanceof Map<?, ?> detail) {
            Object reference = detail.get("ref");
            if (!(reference instanceof String ref)) {
                throw new IllegalArgumentException("catalog entry `" + alias
                        + "` needs a version or version.ref");
            }
            Object versionsValue = catalog.get("versions");
            if (!(versionsValue instanceof Map<?, ?> versions)
                    || !(versions.get(ref) instanceof String referencedVersion)) {
                throw new IllegalArgumentException("catalog entry `" + alias
                        + "` references missing version `" + ref + "`");
            }
            version = referencedVersion;
        }
        if (version == null || version.isBlank()) {
            throw new IllegalArgumentException("catalog entry `" + alias
                    + "` needs a pinned version in " + manifest);
        }
        return module + ":" + version;
    }
}
