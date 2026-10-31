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
    Files.writeString(
        manifest,
        """
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
    assertEquals(List.of("src/bench/java"), config.benchmarks().sourceDirectories());
    assertEquals("1.37", config.benchmarks().jmhVersion());
  }

  @Test
  void readsConfiguredJmhVersionSourcesAndDependencies() throws Exception {
    Path manifest = temp.resolve("jot.toml");
    Files.writeString(
        manifest,
        """
        [project]
        name = "bench-config"
        [toolchains]
        java = "27"
        [bench]
        jmh-version = "1.38"
        source-dirs = ["benchmarks/java"]
        deps = ["org.example:fixture:1.0"]
        """);

    BenchmarkConfig bench = ProjectConfig.read(manifest, null).benchmarks();

    assertEquals("1.38", bench.jmhVersion());
    assertEquals(List.of("benchmarks/java"), bench.sourceDirectories());
    assertEquals(List.of("org.example:fixture:1.0"), bench.dependencies());
  }

  @Test
  void readsDependenciesDeclaredAsTomlTables() throws Exception {
    Path manifest = temp.resolve("jot.toml");
    Files.writeString(
        manifest,
        """
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
  void readsAnnotationProcessorCoordinatesAndOptions() throws Exception {
    Path manifest = temp.resolve("jot.toml");
    Files.writeString(
        manifest,
        """
        [project]
        name = "processor-config"
        [toolchains]
        java = "27"
        [processors.example]
        coords = "org.example:processor:1.0"
        options = { greeting = "hello from processor" }
        """);

    ProjectConfig config = ProjectConfig.read(manifest, null);

    assertEquals("org.example:processor:1.0", config.processors().get("example"));
    assertEquals("hello from processor", config.processorOptions().get("greeting"));
  }

  @Test
  void readsExplicitJavaFormatterConfiguration() throws Exception {
    Path manifest = temp.resolve("jot.toml");
    Files.writeString(
        manifest,
        """
        [project]
        name = "formatted"
        [toolchains]
        java = "27"
        [format]
        java = "com.google.googlejavaformat:google-java-format:1.37.0"
        style = "aosp"
        """);

    ProjectConfig config = ProjectConfig.read(manifest, null);

    assertEquals("com.google.googlejavaformat:google-java-format:1.37.0", config.javaFormatter());
    assertEquals("aosp", config.javaFormatterStyle());
  }

  @Test
  void resolvesVersionCatalogAliasesFromWorkspaceRoot() throws Exception {
    Files.writeString(
        temp.resolve("libs.versions.toml"),
        """
        [versions]
        logging = "2.0.16"
        [libraries]
        slf4j-api = { module = "org.slf4j:slf4j-api", version.ref = "logging" }
        """);
    Path manifest = temp.resolve("module/jot.toml");
    Files.createDirectories(manifest.getParent());
    Files.writeString(
        manifest,
        """
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
