package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ProjectInitializerTest {
  @TempDir Path temp;

  @Test
  void createsAnApplicationThatCompilesAndRuns() throws Exception {
    Path root =
        ProjectInitializer.initialize(
            temp.resolve("hello-app"), "hello-app", "app", "example.hello");
    ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);
    WorkspaceProject project = ProjectWorkspace.loadProject(root, config).find("hello-app");

    BuildEngine.build(project);

    Process process =
        new ProcessBuilder(
                javaExecutable(),
                "-cp",
                root.resolve("target/classes").toString(),
                "example.hello.Main",
                "Ada")
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());
    assertEquals(0, process.waitFor());
    assertEquals("Hello, Ada!", output.strip());
    assertTrue(Files.isRegularFile(root.resolve("test/example/hello/MainTest.java")));
  }

  @Test
  void createsLibrariesWithoutRunnableMainClasses() throws Exception {
    Path root =
        ProjectInitializer.initialize(temp.resolve("greetings"), "greetings", "library", null);
    ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);

    assertEquals("", config.mainClass());
    assertTrue(Files.isRegularFile(root.resolve("src/app/greetings/Greeting.java")));
    assertTrue(Files.readString(root.resolve("jot.toml")).contains("java = \"27\""));
  }

  @Test
  void refusesOverwritesAndInvalidPackageNames() throws Exception {
    Path root = temp.resolve("existing");
    ProjectInitializer.initialize(root, "existing", "app", "example.existing");

    assertThrows(
        java.io.IOException.class,
        () -> ProjectInitializer.initialize(root, "existing", "app", "example.existing"));
    assertThrows(
        IllegalArgumentException.class,
        () -> ProjectInitializer.initialize(temp.resolve("bad"), "bad", "app", "bad.class"));
    assertTrue(!Files.exists(temp.resolve("bad/jot.toml")));
  }

  private String javaExecutable() {
    String executable = System.getProperty("os.name").startsWith("Windows") ? "java.exe" : "java";
    return Path.of(System.getProperty("java.home"), "bin", executable).toString();
  }
}
