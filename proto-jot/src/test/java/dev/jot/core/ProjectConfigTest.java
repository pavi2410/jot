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
}
