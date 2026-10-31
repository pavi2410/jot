package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.net.URI;
import java.net.URLClassLoader;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
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
    assertTrue(Files.isRegularFile(root.resolve("src/test/java/example/hello/MainTest.java")));
  }

  @Test
  void createsLibrariesWithoutRunnableMainClasses() throws Exception {
    Path root =
        ProjectInitializer.initialize(temp.resolve("greetings"), "greetings", "library", null);
    ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);

    assertEquals("", config.mainClass());
    assertTrue(Files.isRegularFile(root.resolve("src/main/java/com/example/Greeting.java")));
    assertEquals(java.util.List.of("src/main/java"), config.sourceDirectories());
    assertEquals(java.util.List.of("src/test/java"), config.testSourceDirectories());
    assertTrue(Files.readString(root.resolve("jot.toml")).contains("java = \"27\""));
  }

  @Test
  void createsAJavaCliTemplateWithGroupPackageAndPicocli() throws Exception {
    Path root =
        ProjectInitializer.initialize(
            temp.resolve("sample-cli"), "sample-cli", "java-cli", "org.example.tools", null);
    String manifest = Files.readString(root.resolve("jot.toml"));
    String source = Files.readString(root.resolve("src/main/java/org/example/tools/Main.java"));

    assertTrue(manifest.contains("group = \"org.example.tools\""));
    assertTrue(manifest.contains("layout = \"maven\""));
    assertTrue(manifest.contains("picocli = \"info.picocli:picocli:4.7.6\""));
    assertTrue(source.contains("import picocli.CommandLine;"));
    assertTrue(source.contains("--name"));
    ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);
    assertTrue(config.dependencies().containsKey("picocli"));
  }

  @Test
  void createsAJavaServerTemplateThatServesHealthRequests() throws Exception {
    Path root =
        ProjectInitializer.initialize(
            temp.resolve("health-server"), "health-server", "java-server", "org.example", null);
    ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);
    WorkspaceProject project = ProjectWorkspace.loadProject(root, config).find("health-server");
    BuildEngine.build(project);

    try (URLClassLoader loader =
        new URLClassLoader(
            new java.net.URL[] {root.resolve("target/classes").toUri().toURL()},
            ProjectInitializerTest.class.getClassLoader())) {
      Class<?> serverType = loader.loadClass("org.example.ServerMain");
      com.sun.net.httpserver.HttpServer server =
          (com.sun.net.httpserver.HttpServer)
              serverType.getMethod("createServer", int.class).invoke(null, 0);
      try {
        server.start();
        URI endpoint = URI.create("http://localhost:" + server.getAddress().getPort() + "/health");
        HttpResponse<String> response =
            HttpClient.newHttpClient()
                .send(
                    HttpRequest.newBuilder(endpoint).GET().build(),
                    HttpResponse.BodyHandlers.ofString());
        assertEquals(200, response.statusCode());
        assertEquals("ok", response.body());
      } finally {
        server.stop(0);
      }
    }
  }

  @Test
  void createsBuildableMultiModuleJavaWorkspace() throws Exception {
    Path root =
        ProjectInitializer.initialize(
            temp.resolve("shopflow"), "shopflow", "java-workspace", "org.example.shopflow", null);
    ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);
    ProjectWorkspace workspace = ProjectWorkspace.load(root, config);

    assertEquals(
        java.util.List.of("shopflow-domain", "shopflow-api", "shopflow-cli"),
        workspace.ordered().stream().map(WorkspaceProject::name).toList());
    for (WorkspaceProject project : workspace.ordered()) BuildEngine.build(project);
    assertTrue(Files.isRegularFile(root.resolve("cli/target/shopflow-cli-0.1.0.jar")));
    assertTrue(Files.isRegularFile(root.resolve("api/target/shopflow-api-0.1.0.jar")));
    assertTrue(Files.isRegularFile(root.resolve("domain/target/shopflow-domain-0.1.0.jar")));
    Process process =
        new ProcessBuilder(
                javaExecutable(),
                "-cp",
                System.getProperty("java.class.path"),
                "dev.jot.cli.Main",
                "run",
                "--project",
                root.toString(),
                "--module",
                "cli",
                "--",
                "Ada")
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());
    assertEquals(0, process.waitFor(), output);
    assertTrue(output.contains("generated order for Ada"), output);
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
    return Path.of(System.getProperty("java.home"), "bin", Platform.executableName("java"))
        .toString();
  }
}
