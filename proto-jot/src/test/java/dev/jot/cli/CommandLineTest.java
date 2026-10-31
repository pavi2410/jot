package dev.jot.cli;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class CommandLineTest {
  @TempDir Path temp;

  @Test
  void separatesJotOptionsFromApplicationArguments() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {
              "run", "--project", "demo", "--module", "app", "--offline", "--", "--port", "8080"
            });

    assertEquals("run", parsed.command());
    assertEquals("app", parsed.module());
    assertEquals(List.of("--port", "8080"), parsed.runArguments());
    assertTrue(parsed.projectRoot().endsWith("demo"));
    assertTrue(parsed.offline());
  }

  @Test
  void acceptsOfflineAsAGlobalOptionBeforeTheCommand() {
    CommandLine parsed = CommandLineParser.parse(new String[] {"--offline", "--offline", "build"});

    assertEquals("build", parsed.command());
    assertTrue(parsed.offline());
  }

  @Test
  void parsesTreeCoordinateAndDepth() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {"tree", "org.example:library:1.2", "--depth", "3", "--offline"});

    assertEquals("tree", parsed.command());
    assertEquals("org.example:library:1.2", parsed.dependencyCoordinate());
    assertEquals(3, parsed.treeDepth());
    assertTrue(parsed.offline());
    assertEquals("tree needs a dependency coordinate or --workspace", parseError("tree"));
    assertEquals(
        "--depth needs a non-negative integer", parseError("tree", "x:y:1", "--depth", "deep"));
  }

  @Test
  void parsesWorkspaceTreeSelectionAndRejectsConflictingInputs() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {"tree", "--workspace", "--module", "app", "--depth", "2"});

    assertEquals("tree", parsed.command());
    assertTrue(parsed.treeWorkspace());
    assertEquals("app", parsed.module());
    assertEquals(2, parsed.treeDepth());
    assertEquals(
        "tree dependency cannot be combined with --workspace",
        parseError("tree", "org.example:library:1.0", "--workspace"));
  }

  @Test
  void parsesJavaToolchainCommands() {
    CommandLine parsed =
        CommandLineParser.parse(new String[] {"toolchain", "pin", "java@21", "--workspace"});

    assertEquals("toolchain", parsed.command());
    assertEquals("pin", parsed.toolchainAction());
    assertEquals("java@21", parsed.toolchainSpec());
    assertTrue(parsed.workspacePin());
    assertEquals("toolchain needs list, pin, or install", parseError("toolchain"));

    CommandLine install =
        CommandLineParser.parse(
            new String[] {"toolchain", "install", "java@21", "--force", "--offline"});
    assertEquals("install", install.toolchainAction());
    assertEquals("java@21", install.toolchainSpec());
    assertTrue(install.toolchainForce());
    assertTrue(install.offline());
    assertEquals(
        "--force only applies to toolchain install",
        parseError("toolchain", "pin", "java@21", "--force"));
  }

  @Test
  void pinsJavaToolchainFromTheCli() throws Exception {
    Files.writeString(temp.resolve("jot.toml"), "[project]\nname = \"toolchain-cli\"\n");
    String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
    Process process =
        new ProcessBuilder(
                java,
                "-cp",
                System.getProperty("java.class.path"),
                "dev.jot.cli.Main",
                "toolchain",
                "pin",
                "java@21",
                "--project",
                temp.toString())
            .directory(temp.toFile())
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());

    assertEquals(0, process.waitFor(), output);
    assertTrue(Files.readString(temp.resolve("jot.toml")).contains("java = \"21\""));
    assertTrue(output.contains("pinned java@21"));
  }

  @Test
  void offlineToolchainInstallFailsClearlyWhenTheJdkIsMissing() throws Exception {
    Path home = temp.resolve("empty-home");
    Files.createDirectories(home);
    String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
    Process process =
        new ProcessBuilder(
                java,
                "-Duser.home=" + home,
                "-cp",
                System.getProperty("java.class.path"),
                "dev.jot.cli.Main",
                "toolchain",
                "install",
                "java@21",
                "--offline")
            .directory(temp.toFile())
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());

    assertEquals(1, process.waitFor(), output);
    assertTrue(output.contains("offline mode: cannot install Java 21"));
  }

  @Test
  void parsesResolveWithDirectDependencyListing() {
    CommandLine parsed =
        CommandLineParser.parse(new String[] {"resolve", "org.example:library:1.2", "--deps"});

    assertEquals("resolve", parsed.command());
    assertEquals("org.example:library:1.2", parsed.dependencyCoordinate());
    assertTrue(parsed.resolveDependencies());
    assertEquals("resolve needs a dependency coordinate", parseError("resolve"));
    assertEquals("unknown argument: --deps", parseError("build", "--deps"));
  }

  @Test
  void parsesOutdatedWorkspaceSelection() {
    CommandLine parsed =
        CommandLineParser.parse(new String[] {"outdated", "--module", "app", "--offline"});

    assertEquals("outdated", parsed.command());
    assertEquals("app", parsed.module());
    assertTrue(parsed.offline());
  }

  @Test
  void parsesDocumentationWorkspaceSelection() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {"doc", "--project", "demo", "--module", "api", "--offline"});

    assertEquals("doc", parsed.command());
    assertEquals("api", parsed.module());
    assertTrue(parsed.projectRoot().endsWith("demo"));
    assertTrue(parsed.offline());
  }

  @Test
  void parsesLintWorkspaceSelection() {
    CommandLine parsed = CommandLineParser.parse(new String[] {"lint", "--module", "core"});

    assertEquals("lint", parsed.command());
    assertEquals("core", parsed.module());
  }

  @Test
  void parsesFormatterCheckMode() {
    CommandLine parsed = CommandLineParser.parse(new String[] {"fmt", "--check", "--offline"});

    assertEquals("fmt", parsed.command());
    assertTrue(parsed.formatCheck());
    assertTrue(parsed.offline());
    assertEquals("unknown argument: --check", parseError("build", "--check"));
  }

  @Test
  void parsesDependencyAddAndRemoveOptions() {
    CommandLine add =
        CommandLineParser.parse(
            new String[] {"add", "org.example:annotations:1.0", "--name", "annotations", "--test"});
    assertEquals("org.example:annotations:1.0", add.dependencyCoordinate());
    assertEquals("annotations", add.dependencyName());
    assertTrue(add.testDependency());

    CommandLine remove = CommandLineParser.parse(new String[] {"remove", "annotations", "--test"});
    assertEquals("annotations", remove.dependencyName());
    assertTrue(remove.testDependency());

    CommandLine catalog = CommandLineParser.parse(new String[] {"add", "--catalog", "junit"});
    assertEquals("junit", catalog.dependencyCatalog());
  }

  @Test
  void errorsWithoutMessagesStillShowTheirType() {
    assertEquals(
        "java.lang.IllegalStateException", Main.failureMessage(new IllegalStateException()));
  }

  @Test
  void parsesProjectInitializationOptions() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {
              "init", "sample-app", "--template", "library", "--package", "dev.sample"
            });

    assertEquals("init", parsed.command());
    assertEquals("sample-app", parsed.projectName());
    assertEquals("library", parsed.template());
    assertEquals("dev.sample", parsed.packageName());
  }

  @Test
  void rejectsMissingOptionValuesAndInitOptionsOnOtherCommands() {
    assertEquals("--project needs a value", parseError("build", "--project"));
    assertEquals("unknown argument: --template", parseError("build", "--template", "app"));
    assertEquals(
        "use either a dependency coordinate or --catalog",
        parseError("add", "org.example:lib:1.0", "--catalog", "lib"));
  }

  private static String parseError(String... arguments) {
    try {
      CommandLineParser.parse(arguments);
      throw new AssertionError("expected argument parsing to fail");
    } catch (IllegalArgumentException error) {
      return error.getMessage();
    }
  }

  @Test
  void infersValidProjectNamesFromDirectoryNames() {
    assertEquals(
        "jot-init-smoke_abc123", Main.inferredProjectName(Path.of("/tmp/jot-init.smoke_abc123")));
    assertEquals("project-123-demo", Main.inferredProjectName(Path.of("/tmp/123.demo")));
  }
}
