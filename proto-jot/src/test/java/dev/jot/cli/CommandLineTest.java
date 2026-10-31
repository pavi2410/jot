package dev.jot.cli;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;

class CommandLineTest {
  @Test
  void separatesJotOptionsFromApplicationArguments() {
    CommandLine parsed =
        CommandLine.parse(
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
    CommandLine parsed = CommandLine.parse(new String[] {"--offline", "build"});

    assertEquals("build", parsed.command());
    assertTrue(parsed.offline());
  }

  @Test
  void parsesTreeCoordinateAndDepth() {
    CommandLine parsed =
        CommandLine.parse(
            new String[] {"tree", "org.example:library:1.2", "--depth", "3", "--offline"});

    assertEquals("tree", parsed.command());
    assertEquals("org.example:library:1.2", parsed.dependencyCoordinate());
    assertEquals(3, parsed.treeDepth());
    assertTrue(parsed.offline());
    assertEquals("tree needs a dependency coordinate", parseError("tree"));
    assertEquals(
        "--depth needs a non-negative integer", parseError("tree", "x:y:1", "--depth", "deep"));
  }

  @Test
  void parsesOutdatedWorkspaceSelection() {
    CommandLine parsed =
        CommandLine.parse(new String[] {"outdated", "--module", "app", "--offline"});

    assertEquals("outdated", parsed.command());
    assertEquals("app", parsed.module());
    assertTrue(parsed.offline());
  }

  @Test
  void parsesDocumentationWorkspaceSelection() {
    CommandLine parsed =
        CommandLine.parse(
            new String[] {"doc", "--project", "demo", "--module", "api", "--offline"});

    assertEquals("doc", parsed.command());
    assertEquals("api", parsed.module());
    assertTrue(parsed.projectRoot().endsWith("demo"));
    assertTrue(parsed.offline());
  }

  @Test
  void parsesLintWorkspaceSelection() {
    CommandLine parsed = CommandLine.parse(new String[] {"lint", "--module", "core"});

    assertEquals("lint", parsed.command());
    assertEquals("core", parsed.module());
  }

  @Test
  void parsesFormatterCheckMode() {
    CommandLine parsed = CommandLine.parse(new String[] {"fmt", "--check", "--offline"});

    assertEquals("fmt", parsed.command());
    assertTrue(parsed.formatCheck());
    assertTrue(parsed.offline());
    assertEquals("unknown argument: --check", parseError("build", "--check"));
  }

  @Test
  void parsesDependencyAddAndRemoveOptions() {
    CommandLine add =
        CommandLine.parse(
            new String[] {"add", "org.example:annotations:1.0", "--name", "annotations", "--test"});
    assertEquals("org.example:annotations:1.0", add.dependencyCoordinate());
    assertEquals("annotations", add.dependencyName());
    assertTrue(add.testDependency());

    CommandLine remove = CommandLine.parse(new String[] {"remove", "annotations", "--test"});
    assertEquals("annotations", remove.dependencyName());
    assertTrue(remove.testDependency());

    CommandLine catalog = CommandLine.parse(new String[] {"add", "--catalog", "junit"});
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
        CommandLine.parse(
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
      CommandLine.parse(arguments);
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
