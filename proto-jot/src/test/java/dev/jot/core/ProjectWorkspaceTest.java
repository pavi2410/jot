package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ProjectWorkspaceTest {
  @TempDir Path temp;

  @Test
  void ordersModulesByLocalDependenciesAndInheritsTheJavaToolchain() throws Exception {
    write(
        "jot.toml",
        """
        [workspace]
        members = ["app", "core"]
        group = "org.example"
        [toolchains]
        java = "27"
        """);
    write(
        "core/jot.toml",
        """
        [project]
        name = "jot-core"
        """);
    write(
        "app/jot.toml",
        """
        [project]
        name = "jot-app"
        main-class = "sample.Main"
        [dependencies]
        core = { path = "../core" }
        """);

    ProjectConfig root = ProjectConfig.read(temp.resolve("jot.toml"), null);
    ProjectWorkspace workspace = ProjectWorkspace.load(temp, root);

    assertEquals(
        List.of("jot-core", "jot-app"),
        workspace.ordered().stream().map(WorkspaceProject::name).toList());
    assertEquals("27", workspace.find("app").config().javaVersion());
    assertEquals("org.example", workspace.find("app").config().group());
    assertEquals("jot-core", workspace.find("app").dependencies().getFirst().name());
    assertEquals("jot-app", workspace.selectRunnable("app").name());
  }

  @Test
  void reportsCyclesInLocalDependencies() throws Exception {
    write(
        "jot.toml",
        """
        [workspace]
        members = ["alpha", "beta"]
        [toolchains]
        java = "27"
        """);
    write(
        "alpha/jot.toml",
        """
        [project]
        name = "alpha"
        [dependencies]
        beta = { path = "../beta" }
        """);
    write(
        "beta/jot.toml",
        """
        [project]
        name = "beta"
        [dependencies]
        alpha = { path = "../alpha" }
        """);

    ProjectConfig root = ProjectConfig.read(temp.resolve("jot.toml"), null);
    IllegalArgumentException error =
        assertThrows(IllegalArgumentException.class, () -> ProjectWorkspace.load(temp, root));
    org.junit.jupiter.api.Assertions.assertTrue(error.getMessage().contains("cycle"));
  }

  @Test
  void runsWorkspaceDependencyTreeForOneModule() throws Exception {
    write(
        "jot.toml",
        """
        [workspace]
        members = ["core", "app"]
        [toolchains]
        java = "27"
        """);
    write("core/jot.toml", "[project]\nname = \"core\"\n");
    write(
        "app/jot.toml",
        """
        [project]
        name = "app"
        [dependencies]
        core = { path = "../core" }
        sample = "sample:root:1.0"
        """);
    write(
        "repository/sample/root/1.0/root-1.0.pom",
        """
        <project><groupId>sample</groupId><artifactId>root</artifactId><version>1.0</version>
          <dependencies><dependency><groupId>sample</groupId><artifactId>child</artifactId><version>2.0</version></dependency></dependencies>
        </project>
        """);
    write(
        "repository/sample/child/2.0/child-2.0.pom",
        "<project><groupId>sample</groupId><artifactId>child</artifactId><version>2.0</version></project>");
    Files.createDirectories(temp.resolve("home"));
    String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
    ProcessBuilder command =
        new ProcessBuilder(
                java,
                "-Duser.home=" + temp.resolve("home"),
                "-cp",
                System.getProperty("java.class.path"),
                "dev.jot.cli.Main",
                "tree",
                "--workspace",
                "--project",
                temp.toString(),
                "--module",
                "app",
                "--depth",
                "1")
            .directory(temp.toFile())
            .redirectErrorStream(true);
    command
        .environment()
        .put("JOT_MAVEN_REPOSITORY", temp.resolve("repository").toUri().toString());
    Process process = command.start();
    String output = new String(process.getInputStream().readAllBytes());

    assertTrue(process.waitFor() == 0, output);
    assertTrue(output.contains("workspace"));
    assertTrue(output.contains("app"));
    assertTrue(output.contains("core (workspace)"));
    assertTrue(output.contains("sample:root:1.0"));
    assertTrue(output.contains("sample:child:2.0 [compile]"));
  }

  private void write(String relativePath, String contents) throws Exception {
    Path file = temp.resolve(relativePath);
    Files.createDirectories(file.getParent());
    Files.writeString(file, contents);
  }
}
