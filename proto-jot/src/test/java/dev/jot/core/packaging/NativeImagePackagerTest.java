package dev.jot.core.packaging;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.jot.core.ProjectConfig;
import dev.jot.core.WorkspaceProject;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class NativeImagePackagerTest {
  @TempDir Path temp;

  @Test
  void locatesNativeImageFromGraalvmThenProjectToolchainThenPath() throws Exception {
    Path graal = toolchain("graal");
    Path javaHome = toolchain("java");
    Path pathDirectory = temp.resolve("path entry");
    Path fromPath = executable(pathDirectory.resolve("native-image"));

    assertEquals(
        graal.resolve("bin/native-image"),
        NativeImagePackager.locateNativeImage(
            javaHome, graal.toString(), pathDirectory.toString(), "Mac OS X"));
    assertEquals(
        javaHome.resolve("bin/native-image"),
        NativeImagePackager.locateNativeImage(
            javaHome, null, pathDirectory.toString(), "Mac OS X"));
    assertEquals(
        fromPath.toAbsolutePath().normalize(),
        NativeImagePackager.locateNativeImage(null, null, pathDirectory.toString(), "Mac OS X"));
  }

  @Test
  void reportsHowToInstallNativeImageWhenItCannotBeFound() {
    IOException error =
        assertThrows(
            IOException.class,
            () -> NativeImagePackager.locateNativeImage(null, null, "", "Mac OS X"));

    assertTrue(error.getMessage().contains("GraalVM Native Image was not found"));
    assertTrue(error.getMessage().contains("GRAALVM_HOME"));
  }

  @Test
  void passesJarAndOutputPathsAsIndividualNativeImageArguments() {
    Path tool = temp.resolve("Graal VM/bin/native-image");
    Path jar = temp.resolve("my project/target/bin/app.jar");
    Path output = temp.resolve("my project/target/bin/app");

    assertEquals(
        List.of(
            tool.toString(),
            "--no-fallback",
            "--class-path",
            jar.toAbsolutePath().normalize().toString(),
            "-o",
            output.toAbsolutePath().normalize().toString(),
            "sample.Main"),
        NativeImagePackager.command(tool, jar, "sample.Main", output));
  }

  @Test
  void buildsNativeOutputOnceAndRebuildsWhenTheFatJarChanges() throws Exception {
    Path projectRoot = temp.resolve("native-project");
    Files.createDirectories(projectRoot.resolve("target/bin"));
    Files.writeString(
        projectRoot.resolve("jot.toml"),
        "[project]\nname = \"native-project\"\nmain-class = \"sample.Main\"\n"
            + "[toolchains]\njava = \""
            + Runtime.version().feature()
            + "\"\n");
    Path fatJar = projectRoot.resolve("target/bin/native-project.jar");
    Files.writeString(fatJar, "fat jar v1");
    WorkspaceProject project =
        new WorkspaceProject(
            projectRoot, ProjectConfig.read(projectRoot.resolve("jot.toml"), null), List.of());
    Path invocations = temp.resolve("native-image-builds.txt");
    List<String> launcher =
        List.of(
            Path.of(System.getProperty("java.home"), "bin", "java").toString(),
            "-cp",
            System.getProperty("java.class.path"),
            FakeNativeImage.class.getName(),
            invocations.toString());

    NativeImagePackager.build(project, launcher);
    Path executable = projectRoot.resolve("target/bin/native-project" + executableSuffix());
    assertEquals("native-image-builds=1", Files.readString(invocations));
    assertEquals("native executable", Files.readString(executable));

    NativeImagePackager.build(project, launcher);
    assertEquals("native-image-builds=1", Files.readString(invocations));

    Files.writeString(fatJar, "fat jar v2");
    NativeImagePackager.build(project, launcher);
    assertEquals("native-image-builds=2", Files.readString(invocations));
  }

  @Test
  void rejectsConfiguredGraalvmOlderThanTheProjectBytecode() throws Exception {
    Path projectRoot = temp.resolve("version-mismatch");
    Files.createDirectories(projectRoot);
    Files.writeString(
        projectRoot.resolve("jot.toml"),
        "[project]\nname = \"version-mismatch\"\nmain-class = \"sample.Main\"\n"
            + "[toolchains]\njava = \"27\"\ngraalvm = \"25\"\n");
    WorkspaceProject project =
        new WorkspaceProject(
            projectRoot, ProjectConfig.read(projectRoot.resolve("jot.toml"), null), List.of());

    IOException error = assertThrows(IOException.class, () -> NativeImagePackager.build(project));

    assertTrue(error.getMessage().contains("cannot compile Java 27 bytecode"));
  }

  private String executableSuffix() {
    return System.getProperty("os.name").toLowerCase(java.util.Locale.ROOT).contains("win")
        ? ".exe"
        : "";
  }

  private Path toolchain(String name) throws IOException {
    Path home = temp.resolve(name);
    executable(home.resolve("bin/native-image"));
    return home;
  }

  private Path executable(Path path) throws IOException {
    Files.createDirectories(path.getParent());
    Files.writeString(path, "native-image test stub");
    path.toFile().setExecutable(true);
    return path;
  }
}

final class FakeNativeImage {
  private FakeNativeImage() {}

  public static void main(String[] args) throws Exception {
    Path invocations = Path.of(args[0]);
    if (args[1].equals("--version")) {
      System.out.println("fake GraalVM Native Image 1.0");
      return;
    }
    int count =
        Files.exists(invocations)
            ? Integer.parseInt(Files.readString(invocations).split("=")[1])
            : 0;
    Files.writeString(invocations, "native-image-builds=" + (count + 1));
    for (int index = 1; index < args.length - 1; index++) {
      if (args[index].equals("-o")) {
        Files.writeString(Path.of(args[index + 1]), "native executable");
        return;
      }
    }
    throw new IllegalArgumentException("missing -o option");
  }
}
