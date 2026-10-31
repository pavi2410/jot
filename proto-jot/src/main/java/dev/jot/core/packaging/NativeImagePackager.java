package dev.jot.core.packaging;

import dev.jot.core.GraalVmToolchainManager;
import dev.jot.core.JavaToolchain;
import dev.jot.core.WorkspaceProject;
import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.Properties;
import java.util.stream.Stream;

/** Builds a GraalVM native executable from a project's executable fat JAR. */
public final class NativeImagePackager {
  private NativeImagePackager() {}

  public static void build(WorkspaceProject project) throws Exception {
    var config = project.config();
    Path nativeImage;
    if (config.graalVmVersion() != null) {
      int graalVmRelease = JavaToolchain.parseRelease(config.graalVmVersion());
      int bytecodeRelease = JavaToolchain.parseRelease(config.javaVersion());
      if (graalVmRelease < bytecodeRelease) {
        throw new IOException(
            "configured GraalVM Java "
                + graalVmRelease
                + " cannot compile Java "
                + bytecodeRelease
                + " bytecode; pin a GraalVM release at least as new as toolchains.java");
      }
      Path graalVmHome = GraalVmToolchainManager.installedHome(config.graalVmVersion());
      nativeImage = GraalVmToolchainManager.nativeImageTool(graalVmHome);
    } else {
      Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
      nativeImage =
          locateNativeImage(
              javaHome,
              System.getenv("GRAALVM_HOME"),
              System.getenv("PATH"),
              System.getProperty("os.name"));
    }
    build(project, List.of(nativeImage.toString()));
  }

  static void build(WorkspaceProject project, List<String> nativeImageLauncher) throws Exception {
    var config = project.config();
    if (config.mainClass().isBlank()) {
      throw new IllegalArgumentException(
          "native packaging requires project.main-class in " + project.root().resolve("jot.toml"));
    }
    Path target = project.root().resolve("target").toAbsolutePath().normalize();
    Path fatJar = target.resolve("bin").resolve(config.name() + ".jar");
    if (!Files.isRegularFile(fatJar)) {
      throw new IOException("executable JAR is missing; build the project before native packaging");
    }

    String version = nativeImageVersion(nativeImageLauncher, project.root());
    String fingerprint = fingerprint(fatJar, config.mainClass(), version);
    Path output = target.resolve("bin").resolve(executableName(config.name()));
    Path stateFile = target.resolve(".jot/native-image.properties");
    Properties state = loadState(stateFile);
    if (Files.isRegularFile(output)
        && fingerprint.equals(state.getProperty("fingerprint"))
        && output.toAbsolutePath().normalize().equals(trackedOutput(target, state))) {
      System.out.println("native: " + displayPath(project.root(), output) + " is up to date");
      return;
    }

    Files.createDirectories(output.getParent());
    Path temporaryDirectory = Files.createTempDirectory(output.getParent(), ".jot-native-");
    Path temporaryOutput = temporaryDirectory.resolve(output.getFileName());
    try {
      List<String> temporaryCommand =
          command(nativeImageLauncher, fatJar, config.mainClass(), temporaryOutput);
      Process process =
          new ProcessBuilder(temporaryCommand)
              .directory(project.root().toFile())
              .inheritIO()
              .start();
      int status = process.waitFor();
      if (status != 0) {
        throw new IOException("native-image failed with exit code " + status);
      }
      if (!Files.isRegularFile(temporaryOutput)) {
        throw new IOException("native-image completed without producing " + temporaryOutput);
      }
      moveReplacing(temporaryOutput, output);
    } finally {
      deleteTree(temporaryDirectory);
    }

    state.setProperty("fingerprint", fingerprint);
    state.setProperty("output", target.relativize(output).toString());
    Files.createDirectories(stateFile.getParent());
    try (var stream = Files.newOutputStream(stateFile)) {
      state.store(stream, "Jot GraalVM native-image state.");
    }
    System.out.println(
        "native: "
            + displayPath(project.root(), output)
            + " ("
            + formatFileSize(Files.size(output))
            + ")");
  }

  static Path locateNativeImage(Path javaHome, String graalvmHome, String path, String osName)
      throws IOException {
    boolean windows = osName.toLowerCase(java.util.Locale.ROOT).contains("win");
    String toolName = windows ? "native-image.cmd" : "native-image";
    List<Path> candidates = new ArrayList<>();
    if (graalvmHome != null && !graalvmHome.isBlank()) {
      candidates.add(Path.of(graalvmHome).resolve("bin").resolve(toolName));
      if (windows) candidates.add(Path.of(graalvmHome).resolve("bin/native-image.exe"));
    }
    if (javaHome != null) {
      candidates.add(javaHome.resolve("bin").resolve(toolName));
      if (windows) candidates.add(javaHome.resolve("bin/native-image.exe"));
    }
    if (path != null) {
      for (String entry : path.split(java.util.regex.Pattern.quote(java.io.File.pathSeparator))) {
        if (entry.isBlank()) continue;
        candidates.add(Path.of(entry).resolve(toolName));
        if (windows) candidates.add(Path.of(entry).resolve("native-image.exe"));
      }
    }
    for (Path candidate : candidates) {
      if (Files.isRegularFile(candidate) && (windows || Files.isExecutable(candidate))) {
        return candidate.toAbsolutePath().normalize();
      }
    }
    throw new IOException(
        "GraalVM Native Image was not found. Install GraalVM with the native-image component, "
            + "set GRAALVM_HOME, or add native-image to PATH.");
  }

  static List<String> command(Path nativeImage, Path fatJar, String mainClass, Path output) {
    return command(List.of(nativeImage.toString()), fatJar, mainClass, output);
  }

  static List<String> command(
      List<String> nativeImageLauncher, Path fatJar, String mainClass, Path output) {
    List<String> command = new ArrayList<>(nativeImageLauncher);
    command.addAll(
        List.of(
            "--no-fallback",
            "--class-path",
            fatJar.toAbsolutePath().normalize().toString(),
            "-o",
            output.toAbsolutePath().normalize().toString(),
            mainClass));
    return List.copyOf(command);
  }

  private static String nativeImageVersion(List<String> launcher, Path directory)
      throws IOException, InterruptedException {
    List<String> versionCommand = new ArrayList<>(launcher);
    versionCommand.add("--version");
    Process process =
        new ProcessBuilder(versionCommand)
            .directory(directory.toFile())
            .redirectErrorStream(true)
            .start();
    String version =
        new String(
            process.getInputStream().readAllBytes(), java.nio.charset.StandardCharsets.UTF_8);
    int status = process.waitFor();
    if (status != 0) {
      throw new IOException(
          "cannot run GraalVM Native Image at "
              + launcher.getFirst()
              + " --version (exit code "
              + status
              + ")"
              + (version.isBlank() ? "" : ":\n" + version.strip()));
    }
    return version.strip();
  }

  private static String fingerprint(Path fatJar, String mainClass, String nativeImageVersion)
      throws Exception {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    try (var input = Files.newInputStream(fatJar)) {
      byte[] buffer = new byte[64 * 1024];
      int count;
      while ((count = input.read(buffer)) >= 0) digest.update(buffer, 0, count);
    }
    digest.update(mainClass.getBytes(java.nio.charset.StandardCharsets.UTF_8));
    digest.update(nativeImageVersion.getBytes(java.nio.charset.StandardCharsets.UTF_8));
    digest.update(System.getProperty("os.name").getBytes(java.nio.charset.StandardCharsets.UTF_8));
    digest.update(System.getProperty("os.arch").getBytes(java.nio.charset.StandardCharsets.UTF_8));
    return HexFormat.of().formatHex(digest.digest());
  }

  private static Properties loadState(Path stateFile) throws IOException {
    Properties properties = new Properties();
    if (Files.isRegularFile(stateFile)) {
      try (var input = Files.newInputStream(stateFile)) {
        properties.load(input);
      }
    }
    return properties;
  }

  private static Path trackedOutput(Path target, Properties state) {
    String output = state.getProperty("output");
    if (output == null || output.isBlank()) return null;
    Path resolved = target.resolve(output).normalize();
    return resolved.startsWith(target) && !resolved.equals(target) ? resolved : null;
  }

  private static String executableName(String projectName) {
    return projectName
        + (System.getProperty("os.name").toLowerCase(java.util.Locale.ROOT).contains("win")
            ? ".exe"
            : "");
  }

  private static String displayPath(Path root, Path output) {
    return root.toAbsolutePath()
        .normalize()
        .relativize(output.toAbsolutePath().normalize())
        .toString();
  }

  private static void moveReplacing(Path source, Path destination) throws IOException {
    try {
      Files.move(
          source, destination, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
    } catch (AtomicMoveNotSupportedException ignored) {
      Files.move(source, destination, StandardCopyOption.REPLACE_EXISTING);
    }
  }

  private static void deleteTree(Path root) throws IOException {
    if (!Files.exists(root)) return;
    try (Stream<Path> paths = Files.walk(root)) {
      for (Path path : paths.sorted(java.util.Comparator.reverseOrder()).toList()) {
        Files.delete(path);
      }
    }
  }

  private static String formatFileSize(long bytes) {
    String[] units = {"B", "KiB", "MiB", "GiB", "TiB"};
    double size = bytes;
    int unit = 0;
    while (size >= 1024 && unit < units.length - 1) {
      size /= 1024;
      unit++;
    }
    return unit == 0
        ? String.format(java.util.Locale.ROOT, "%d %s", bytes, units[unit])
        : String.format(java.util.Locale.ROOT, "%.1f %s", size, units[unit]);
  }
}
