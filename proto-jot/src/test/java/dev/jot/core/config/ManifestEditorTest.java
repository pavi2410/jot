package dev.jot.core.config;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.jot.core.ProjectConfig;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ManifestEditorTest {
    @TempDir Path temp;

    @Test
    void addsAndRemovesCoordinateDependenciesWithoutDroppingComments() throws Exception {
        Path manifest = temp.resolve("jot.toml");
        Files.writeString(manifest, """
                # project comment stays intact
                [project]
                name = "manifest-edit"
                [toolchains]
                java = "27"
                [dependencies]
                legacy = "org.example:legacy:1.0"

                [test-dependencies]
                junit = "org.junit.jupiter:junit-jupiter:5.11.0"
                """);

        ManifestEditor.addDependency(manifest, "lib=core", "org.example:core:2.0", false);

        ProjectConfig config = ProjectConfig.read(manifest, null);
        assertEquals("org.example:core:2.0", config.dependencies().get("lib=core"));
        assertEquals("org.example:legacy:1.0", config.dependencies().get("legacy"));
        assertTrue(Files.readString(manifest).contains("# project comment stays intact"));
        assertTrue(Files.readString(manifest).contains("\"lib=core\" = \"org.example:core:2.0\""));

        assertTrue(ManifestEditor.removeDependency(manifest, "lib=core", false));
        assertFalse(ManifestEditor.removeDependency(manifest, "lib=core", false));
        assertEquals(false, ProjectConfig.read(manifest, null).dependencies().containsKey("lib=core"));
    }

    @Test
    void removesDependenciesDeclaredAsTables() throws Exception {
        Path manifest = temp.resolve("jot.toml");
        Files.writeString(manifest, """
                [project]
                name = "nested-dependency"
                [toolchains]
                java = "27"
                [dependencies.local]
                path = "../local"
                """);

        assertTrue(ManifestEditor.removeDependency(manifest, "local", false));
        assertTrue(ProjectConfig.read(manifest, null).pathDependencies().isEmpty());
    }
}
