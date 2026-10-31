package dev.jot.cli;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.jot.core.ProjectInitializer;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
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

    CommandLine.Run run = assertInstanceOf(CommandLine.Run.class, parsed.command());
    assertEquals("app", run.module());
    assertEquals(List.of("--port", "8080"), run.arguments());
    assertTrue(parsed.projectRoot().endsWith("demo"));
    assertTrue(parsed.offline());
  }

  @Test
  void acceptsOfflineAsAGlobalOptionBeforeTheCommand() {
    CommandLine parsed = CommandLineParser.parse(new String[] {"--offline", "--offline", "build"});

    assertInstanceOf(CommandLine.Build.class, parsed.command());
    assertTrue(parsed.offline());
  }

  @Test
  void representsSimpleCommandsAsTheirOwnRecords() {
    CommandLine.Build build =
        assertInstanceOf(
            CommandLine.Build.class,
            CommandLineParser.parse(new String[] {"build", "--module", "core"}).command());
    assertEquals("core", build.module());
    assertInstanceOf(
        CommandLine.Test.class, CommandLineParser.parse(new String[] {"test"}).command());
    assertInstanceOf(
        CommandLine.Deps.class, CommandLineParser.parse(new String[] {"deps"}).command());
    assertInstanceOf(
        CommandLine.Lock.class, CommandLineParser.parse(new String[] {"lock"}).command());
    assertInstanceOf(
        CommandLine.Clean.class, CommandLineParser.parse(new String[] {"clean"}).command());
    CommandLine.Clean globalClean =
        assertInstanceOf(
            CommandLine.Clean.class,
            CommandLineParser.parse(new String[] {"clean", "--global"}).command());
    assertTrue(globalClean.global());
  }

  @Test
  void parsesAdHocLockCoordinatesDepthAndOutput() {
    CommandLine.Lock lock =
        assertInstanceOf(
            CommandLine.Lock.class,
            CommandLineParser.parse(
                    new String[] {
                      "lock",
                      "sample:api:1.0",
                      "sample:util:2.0",
                      "--depth",
                      "4",
                      "--output",
                      "build/custom.lock"
                    })
                .command());

    assertEquals(List.of("sample:api:1.0", "sample:util:2.0"), lock.dependencies());
    assertEquals(4, lock.depth());
    assertEquals("build/custom.lock", lock.output());
    assertEquals("--depth needs a non-negative integer", parseError("lock", "--depth", "-1"));
  }

  @Test
  void adHocLockDoesNotRequireAProjectManifest() {
    IllegalArgumentException error =
        assertThrows(
            IllegalArgumentException.class, () -> Main.execute(new String[] {"lock", "bad"}));

    assertTrue(error.getMessage().startsWith("expected group:artifact:version"));
  }

  @Test
  void parsesTreeCoordinateAndDepth() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {"tree", "org.example:library:1.2", "--depth", "3", "--offline"});

    CommandLine.Tree tree = assertInstanceOf(CommandLine.Tree.class, parsed.command());
    assertEquals("org.example:library:1.2", tree.coordinate());
    assertEquals(3, tree.depth());
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

    CommandLine.Tree tree = assertInstanceOf(CommandLine.Tree.class, parsed.command());
    assertTrue(tree.workspace());
    assertEquals("app", tree.module());
    assertEquals(2, tree.depth());
    assertEquals(
        "tree dependency cannot be combined with --workspace",
        parseError("tree", "org.example:library:1.0", "--workspace"));
  }

  @Test
  void parsesJavaToolchainCommands() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {"toolchain", "pin", "java@corretto-21", "--workspace"});

    CommandLine.Toolchain toolchain =
        assertInstanceOf(CommandLine.Toolchain.class, parsed.command());
    CommandLine.PinToolchain pin =
        assertInstanceOf(CommandLine.PinToolchain.class, toolchain.action());
    assertEquals("java@corretto-21", pin.specification());
    assertTrue(pin.workspace());
    assertEquals("toolchain needs list, pin, or install", parseError("toolchain"));

    CommandLine install =
        CommandLineParser.parse(
            new String[] {"toolchain", "install", "java@zulu-21", "--force", "--offline"});
    CommandLine.Toolchain installCommand =
        assertInstanceOf(CommandLine.Toolchain.class, install.command());
    CommandLine.InstallToolchain installAction =
        assertInstanceOf(CommandLine.InstallToolchain.class, installCommand.action());
    assertEquals("java@zulu-21", installAction.specification());
    assertTrue(installAction.force());
    assertTrue(install.offline());
    assertEquals(
        "--force only applies to toolchain install",
        parseError("toolchain", "pin", "java@21", "--force"));
  }

  @Test
  void parsesSelfManagementSubcommands() {
    CommandLine.Self update =
        assertInstanceOf(
            CommandLine.Self.class,
            CommandLineParser.parse(new String[] {"self", "update", "--version", "1.2.3", "--yes"})
                .command());
    CommandLine.SelfUpdate updateAction =
        assertInstanceOf(CommandLine.SelfUpdate.class, update.action());
    assertEquals("1.2.3", updateAction.version());
    assertTrue(updateAction.yes());

    CommandLine.Self uninstall =
        assertInstanceOf(
            CommandLine.Self.class,
            CommandLineParser.parse(new String[] {"self", "uninstall", "--yes"}).command());
    assertTrue(assertInstanceOf(CommandLine.SelfUninstall.class, uninstall.action()).yes());
    assertEquals("self needs update or uninstall", parseError("self"));
  }

  @Test
  void supportsHelpFlagsOnTopLevelAndNestedSubcommands() {
    List<String> commands =
        List.of(
            "init",
            "build",
            "run",
            "test",
            "bench",
            "audit",
            "doc",
            "lint",
            "fmt",
            "add",
            "remove",
            "deps",
            "outdated",
            "resolve",
            "tree",
            "toolchain",
            "lock",
            "clean",
            "self",
            "publish");
    for (String command : commands) {
      CommandLine help = CommandLineParser.parse(new String[] {command, "--help"});
      assertTrue(help.help(), command);
      assertEquals(command, help.helpTopic());
    }

    assertEquals(
        "toolchain install",
        CommandLineParser.parse(new String[] {"toolchain", "install", "java@27", "-h"})
            .helpTopic());
    assertEquals(
        "self update",
        CommandLineParser.parse(new String[] {"self", "update", "--version", "1.2.3", "--help"})
            .helpTopic());
    assertEquals(
        "toolchain list",
        CommandLineParser.parse(new String[] {"toolchain", "list", "--help"}).helpTopic());
    assertEquals(
        "toolchain pin",
        CommandLineParser.parse(new String[] {"toolchain", "pin", "java@27", "--help"})
            .helpTopic());
    assertEquals(
        "self uninstall",
        CommandLineParser.parse(new String[] {"self", "uninstall", "--help"}).helpTopic());
    assertTrue(
        CommandLineParser.parse(new String[] {"run", "--", "--help"}).command()
            instanceof CommandLine.Run);
  }

  @Test
  void generatesConciseRootAndCommandSpecificHelp() {
    String rootHelp = HelpPrinter.render("", "1.2.3");
    assertTrue(rootHelp.contains("Usage: jot [OPTIONS] <COMMAND> [ARGS]"));
    assertTrue(rootHelp.contains("build"));
    assertTrue(rootHelp.contains("Compile and package the current project"));
    assertTrue(!rootHelp.contains("<init|build|run|"));
    assertTrue(!rootHelp.contains("--forks"));
    assertEquals(rootHelp.indexOf("-h, --help"), rootHelp.lastIndexOf("-h, --help"));

    String buildHelp = HelpPrinter.render("build", "1.2.3");
    assertTrue(buildHelp.contains("Usage: jot build [OPTIONS]"));
    assertTrue(buildHelp.contains("--module <name>"));
    assertTrue(buildHelp.contains("-h, --help"));

    String toolchainHelp = HelpPrinter.render("toolchain", "1.2.3");
    assertTrue(toolchainHelp.contains("install  Install a Java toolchain"));
    assertTrue(HelpPrinter.render("self update", "1.2.3").contains("--version <version>"));
  }

  @Test
  void buildCommandPrintsElapsedBuildTime() throws Exception {
    Path project = temp.resolve("timed-build");
    ProjectInitializer.initialize(project, "timed-build", null, null, null);
    PrintStream originalOut = System.out;
    ByteArrayOutputStream output = new ByteArrayOutputStream();
    try (PrintStream captured = new PrintStream(output)) {
      System.setOut(captured);
      Main.execute(new String[] {"build", "--project", project.toString(), "--offline"});
    } finally {
      System.setOut(originalOut);
    }

    assertTrue(
        output
            .toString()
            .matches(
                "(?s).*-> target/timed-build-0.1.0.jar \\(\\d+(?:\\.\\d)?"
                    + " (?:B|KiB|MiB|GiB|TiB)\\).*"));
    assertTrue(output.toString().matches("(?s).*build: finished in \\d+\\.\\d{3} s\\R?$"));
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
                "java@corretto-21",
                "--project",
                temp.toString())
            .directory(temp.toFile())
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());

    assertEquals(0, process.waitFor(), output);
    assertTrue(
        Files.readString(temp.resolve("jot.toml"))
            .contains("java = { version = \"21\", vendor = \"corretto\" }"));
    assertTrue(output.contains("pinned java@corretto-21"));
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

    CommandLine.Resolve resolve = assertInstanceOf(CommandLine.Resolve.class, parsed.command());
    assertEquals("org.example:library:1.2", resolve.coordinate());
    assertTrue(resolve.dependencies());
    assertEquals("resolve needs a dependency coordinate", parseError("resolve"));
    assertEquals("unknown argument: --deps", parseError("build", "--deps"));
  }

  @Test
  void parsesOutdatedWorkspaceSelection() {
    CommandLine parsed =
        CommandLineParser.parse(new String[] {"outdated", "--module", "app", "--offline"});

    CommandLine.Outdated outdated = assertInstanceOf(CommandLine.Outdated.class, parsed.command());
    assertEquals("app", outdated.module());
    assertTrue(parsed.offline());
  }

  @Test
  void parsesDocumentationWorkspaceSelection() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {"doc", "--project", "demo", "--module", "api", "--open", "--offline"});

    CommandLine.Doc doc = assertInstanceOf(CommandLine.Doc.class, parsed.command());
    assertEquals("api", doc.module());
    assertTrue(doc.open());
    assertTrue(parsed.projectRoot().endsWith("demo"));
    assertTrue(parsed.offline());
  }

  @Test
  void parsesLintWorkspaceSelection() {
    CommandLine parsed = CommandLineParser.parse(new String[] {"lint", "--module", "core"});

    CommandLine.Lint lint = assertInstanceOf(CommandLine.Lint.class, parsed.command());
    assertEquals("core", lint.module());
  }

  @Test
  void parsesFormatterCheckMode() {
    CommandLine parsed = CommandLineParser.parse(new String[] {"fmt", "--check", "--offline"});

    CommandLine.Fmt fmt = assertInstanceOf(CommandLine.Fmt.class, parsed.command());
    assertTrue(fmt.check());
    assertTrue(parsed.offline());
    assertEquals("unknown argument: --check", parseError("build", "--check"));
  }

  @Test
  void parsesBenchmarkFilterAndJmhOptions() {
    CommandLine parsed =
        CommandLineParser.parse(
            new String[] {
              "bench",
              ".*Sort.*",
              "--module",
              "core",
              "--forks",
              "2",
              "--warmup",
              "3",
              "--iterations",
              "4"
            });
    CommandLine.Bench bench = assertInstanceOf(CommandLine.Bench.class, parsed.command());
    assertEquals(".*Sort.*", bench.filter());
    assertEquals("core", bench.module());
    assertEquals(2, bench.forks());
    assertEquals(3, bench.warmup());
    assertEquals(4, bench.iterations());
    assertEquals("--forks needs a non-negative integer", parseError("bench", "--forks", "-1"));
    assertEquals("unknown argument: --forks", parseError("build", "--forks", "2"));
  }

  @Test
  void parsesAuditFixAndCiOptions() {
    CommandLine.Audit audit =
        assertInstanceOf(
            CommandLine.Audit.class,
            CommandLineParser.parse(new String[] {"audit", "--fix", "--ci"}).command());
    assertTrue(audit.fix());
    assertTrue(audit.ci());
    assertEquals("unknown argument: --ci", parseError("build", "--ci"));
  }

  @Test
  void parsesMavenPublicationOptions() {
    CommandLine.Publish publish =
        assertInstanceOf(
            CommandLine.Publish.class,
            CommandLineParser.parse(
                    new String[] {
                      "publish",
                      "--module",
                      "api",
                      "--repository",
                      "file:///tmp/maven-repo",
                      "--username",
                      "publisher",
                      "--password",
                      "secret",
                      "--signing-key",
                      "ABCD",
                      "--dry-run",
                      "--allow-snapshot"
                    })
                .command());
    assertEquals("api", publish.module());
    assertEquals("file:///tmp/maven-repo", publish.repository());
    assertEquals("publisher", publish.username());
    assertEquals("secret", publish.password());
    assertEquals("ABCD", publish.signingKey());
    assertTrue(publish.dryRun());
    assertTrue(publish.allowSnapshot());
    assertEquals("unknown argument: --repository", parseError("build", "--repository", "x"));
  }

  @Test
  void parsesDependencyAddAndRemoveOptions() {
    CommandLine.Add add =
        assertInstanceOf(
            CommandLine.Add.class,
            CommandLineParser.parse(
                    new String[] {
                      "add", "org.example:annotations:1.0", "--name", "annotations", "--test"
                    })
                .command());
    assertEquals("org.example:annotations:1.0", add.coordinate());
    assertEquals("annotations", add.name());
    assertTrue(add.test());

    CommandLine.Remove remove =
        assertInstanceOf(
            CommandLine.Remove.class,
            CommandLineParser.parse(new String[] {"remove", "annotations", "--test"}).command());
    assertEquals("annotations", remove.name());
    assertTrue(remove.test());

    CommandLine.Add catalog =
        assertInstanceOf(
            CommandLine.Add.class,
            CommandLineParser.parse(new String[] {"add", "--catalog", "junit"}).command());
    assertEquals("junit", catalog.catalog());
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
              "init",
              "sample-app",
              "--template",
              "library",
              "--group",
              "org.sample",
              "--package",
              "dev.sample"
            });

    CommandLine.Init init = assertInstanceOf(CommandLine.Init.class, parsed.command());
    assertEquals("sample-app", init.name());
    assertEquals("library", init.template());
    assertEquals("org.sample", init.group());
    assertEquals("dev.sample", init.packageName());
    assertEquals("unknown argument: --group", parseError("build", "--group", "org.sample"));
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
