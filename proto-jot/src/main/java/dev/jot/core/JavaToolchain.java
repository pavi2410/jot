package dev.jot.core;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Properties;
import java.util.stream.Stream;

/** Finds an installed JDK for a project's requested Java release. */
public final class JavaToolchain {
  private JavaToolchain() {}

  public static Path javaHome(String requestedRelease) throws IOException {
    Path managedJdks =
        Path.of(System.getProperty("user.home"), ".jot", "jdks").toAbsolutePath().normalize();
    return selectHome(
        requestedRelease,
        Path.of(System.getProperty("java.home")).toAbsolutePath().normalize(),
        Runtime.version().feature(),
        installedJdks(managedJdks));
  }

  static Path selectHome(
      String requestedRelease, Path runningHome, int runningFeature, List<Installation> installed) {
    int requested = parseRelease(requestedRelease);
    for (Installation installation : installed) {
      if (installation.feature() == requested) return installation.home();
    }

    if (runningFeature >= requested) return runningHome;

    throw new IllegalStateException(
        "project requires Java "
            + requested
            + ", but Jot runs on Java "
            + runningFeature
            + " and no managed JDK "
            + requested
            + " is installed; install it with `jot toolchain install java@"
            + requested
            + "` using the current Rust Jot CLI");
  }

  public static List<Installation> installedJdks(Path jdksDirectory) throws IOException {
    if (!Files.isDirectory(jdksDirectory)) return List.of();
    List<Installation> installations = new ArrayList<>();
    try (Stream<Path> files = Files.walk(jdksDirectory, 8)) {
      for (Path releaseFile :
          files
              .filter(path -> path.getFileName().toString().equals("release"))
              .filter(path -> Files.isRegularFile(path.resolveSibling("bin/java")))
              .filter(path -> Files.isRegularFile(path.resolveSibling("bin/javac")))
              .toList()) {
        Path home = releaseFile.getParent();
        Properties release = new Properties();
        try (var input = Files.newInputStream(releaseFile)) {
          release.load(input);
        }
        String version = release.getProperty("JAVA_VERSION");
        if (version == null) continue;
        if (version.length() >= 2 && version.startsWith("\"") && version.endsWith("\"")) {
          version = version.substring(1, version.length() - 1);
        }
        int feature;
        try {
          feature = Runtime.Version.parse(version).feature();
        } catch (IllegalArgumentException ignored) {
          continue;
        }
        installations.add(new Installation(feature, version, home));
      }
    }
    installations.sort(
        Comparator.comparingInt(Installation::feature)
            .thenComparing(Installation::version)
            .reversed());
    return List.copyOf(installations);
  }

  public static int parseRelease(String requestedRelease) {
    int requested;
    try {
      requested = Integer.parseInt(requestedRelease);
    } catch (NumberFormatException error) {
      throw new IllegalArgumentException(
          "toolchains.java must be a Java release number: " + requestedRelease, error);
    }
    if (requested < 1) {
      throw new IllegalArgumentException(
          "toolchains.java must be a positive Java release: " + requestedRelease);
    }
    return requested;
  }

  public static Path tool(Path javaHome, String name) {
    String executable = System.getProperty("os.name").startsWith("Windows") ? name + ".exe" : name;
    Path tool = javaHome.resolve("bin").resolve(executable);
    if (!Files.isExecutable(tool)) {
      throw new IllegalStateException("JDK tool is missing or not executable: " + tool);
    }
    return tool;
  }

  public record Installation(int feature, String version, Path home) {}
}
