package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ProjectConfigTest {
    @TempDir Path temp;

    @Test
    void mavenLayoutProvidesJavaResourceAndTestDefaults() throws Exception {
        Path manifest = temp.resolve("jot.toml");
        Files.writeString(manifest, """
                [project]
                name = "maven-layout"
                layout = "maven"
                [toolchains]
                java = "27"
                """);

        ProjectConfig config = ProjectConfig.read(manifest, null);

        assertEquals(List.of("src/main/java"), config.sourceDirectories());
        assertEquals(List.of("src/main/resources"), config.resourceDirectories());
        assertEquals(List.of("src/test/java"), config.testSourceDirectories());
    }

    @Test
    void readsDependenciesDeclaredAsTomlTables() throws Exception {
        Path manifest = temp.resolve("jot.toml");
        Files.writeString(manifest, """
                [project]
                name = "toml-dependencies"
                [toolchains]
                java = "27"
                [dependencies.annotations]
                coords = "org.example:annotations:1.0"
                [dependencies.local]
                path = "../local"
                """);

        ProjectConfig config = ProjectConfig.read(manifest, null);

        assertEquals("org.example:annotations:1.0", config.dependencies().get("annotations"));
        assertEquals("../local", config.pathDependencies().get("local"));
    }

    @Test
    void resolvesVersionCatalogAliasesFromWorkspaceRoot() throws Exception {
        Files.writeString(temp.resolve("libs.versions.toml"), """
                [versions]
                logging = "2.0.16"
                [libraries]
                slf4j-api = { module = "org.slf4j:slf4j-api", version.ref = "logging" }
                """);
        Path manifest = temp.resolve("module/jot.toml");
        Files.createDirectories(manifest.getParent());
        Files.writeString(manifest, """
                [project]
                name = "catalog-consumer"
                [toolchains]
                java = "27"
                [dependencies]
                logging = { catalog = "slf4j-api" }
                """);

        ProjectConfig config = ProjectConfig.read(manifest, null);

        assertEquals("org.slf4j:slf4j-api:2.0.16", config.dependencies().get("logging"));
    }
}
