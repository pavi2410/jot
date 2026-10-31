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
}
