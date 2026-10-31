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
    Files.writeString(
        manifest,
        """
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
    Files.writeString(
        manifest,
        """
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

  @Test
  void writesCatalogReferencesAsTomlInlineTables() throws Exception {
    Files.writeString(
        temp.resolve("libs.versions.toml"),
        """
        [versions]
        tomlj = "1.1.1"
        [libraries]
        tomlj = { module = "org.tomlj:tomlj", version.ref = "tomlj" }
        """);
    Path manifest = temp.resolve("jot.toml");
    Files.writeString(
        manifest,
        """
        [project]
        name = "catalog-edit"
        [toolchains]
        java = "27"
        """);

    ManifestEditor.addCatalogDependency(manifest, "parser", "tomlj", true);

    assertEquals(
        "tomlj",
        ((java.util.Map<?, ?>) TomlDocument.read(manifest).table("test-dependencies").get("parser"))
            .get("catalog"));
    assertEquals(
        "org.tomlj:tomlj:1.1.1",
        ProjectConfig.read(manifest, null).testDependencies().get("parser"));
  }

  @Test
  void pinsJavaToolchainWithoutDroppingUnrelatedManifestContent() throws Exception {
    Path manifest = temp.resolve("jot.toml");
    Files.writeString(
        manifest,
        """
        [project]
        name = "demo"

        [toolchains]
        java = "17"

        [dependencies]
        annotations = "org.example:annotations:1.0"
        """);

    ManifestEditor.pinJavaToolchain(manifest, "21");

    ProjectConfig parsed = ProjectConfig.read(manifest, null);
    assertEquals("21", parsed.javaVersion());
    String contents = Files.readString(manifest);
    assertTrue(contents.contains("name = \"demo\""));
    assertTrue(contents.contains("annotations = \"org.example:annotations:1.0\""));
  }
}
