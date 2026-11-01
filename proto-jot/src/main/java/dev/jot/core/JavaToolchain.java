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
    return javaHome(requestedRelease, null);
  }

  public static Path javaHome(String requestedRelease, String requestedVendor) throws IOException {
    Path managedJdks =
        Path.of(System.getProperty("user.home"), ".jot", "jdks").toAbsolutePath().normalize();
    String runningHomeValue = System.getProperty("java.home");
    Path runningHome =
        runningHomeValue == null || runningHomeValue.isBlank()
            ? null
            : Path.of(runningHomeValue).toAbsolutePath().normalize();
    return selectHome(
        requestedRelease,
        requestedVendor,
        runningHome,
        Runtime.version().feature(),
        canonicalVendor(System.getProperty("java.vendor")),
        installedJdks(managedJdks));
  }

  static Path selectHome(
      String requestedRelease, Path runningHome, int runningFeature, List<Installation> installed) {
    return selectHome(requestedRelease, null, runningHome, runningFeature, null, installed);
  }

  static Path selectHome(
      String requestedRelease,
      String requestedVendor,
      Path runningHome,
      int runningFeature,
      String runningVendor,
      List<Installation> installed) {
    int requested = parseRelease(requestedRelease);
    for (Installation installation : installed) {
      if (installation.feature() == requested
          && (requestedVendor == null || requestedVendor.equals(installation.vendor())))
        return installation.home();
    }

    if (runningHome != null
        && runningFeature >= requested
        && (requestedVendor == null || requestedVendor.equals(runningVendor))) return runningHome;

    throw new IllegalStateException(
        "project requires Java "
            + requested
            + (requestedVendor == null ? "" : " from " + requestedVendor)
            + ", but Jot runs on Java "
            + runningFeature
            + " and no managed JDK "
            + requested
            + (requestedVendor == null ? "" : " from " + requestedVendor)
            + " is installed; install it with `jot toolchain install java@"
            + (requestedVendor == null ? requested : requestedVendor + "-" + requested)
            + "` using Java Jot");
  }

  public static List<Installation> installedJdks(Path jdksDirectory) throws IOException {
    if (!Files.isDirectory(jdksDirectory)) return List.of();
    List<Installation> installations = new ArrayList<>();
    try (Stream<Path> files = Files.walk(jdksDirectory, 8)) {
      for (Path releaseFile :
          files
              .filter(path -> path.getFileName().toString().equals("release"))
              .filter(path -> Platform.isJdkHome(path.getParent()))
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
        String vendor = managedVendor(home, jdksDirectory);
        if (vendor.equals("unknown")) vendor = canonicalVendor(release.getProperty("IMPLEMENTOR"));
        if (vendor == null) vendor = "unknown";
        installations.add(new Installation(feature, version, home, vendor));
      }
    }
    installations.sort(
        Comparator.comparingInt(Installation::feature)
            .thenComparing(Installation::version)
            .reversed()
            .thenComparing(Installation::vendor));
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

  public static ToolchainRequest parseRequest(String specification) {
    if (specification == null || specification.isBlank())
      throw new IllegalArgumentException("Java toolchain version is required");
    int separator = specification.indexOf('-');
    String vendor = null;
    String version = specification;
    if (separator > 0) {
      String prefix = specification.substring(0, separator).toLowerCase(java.util.Locale.ROOT);
      vendor =
          switch (prefix) {
            case "adoptium", "temurin", "openjdk" -> "adoptium";
            case "corretto" -> "corretto";
            case "zulu" -> "zulu";
            case "oracle" -> "oracle";
            case "graalvm" -> "graalvm";
            default -> null;
          };
      if (vendor != null) version = specification.substring(separator + 1);
    }
    parseRelease(version);
    return new ToolchainRequest(version, vendor);
  }

  private static String canonicalVendor(String vendor) {
    if (vendor == null) return null;
    String value = vendor.toLowerCase(java.util.Locale.ROOT);
    if (value.contains("adoptium") || value.contains("temurin")) return "adoptium";
    if (value.contains("amazon") || value.contains("corretto")) return "corretto";
    if (value.contains("azul") || value.contains("zulu")) return "zulu";
    if (value.contains("oracle")) return "oracle";
    if (value.contains("graalvm")) return "graalvm";
    return value;
  }

  private static String managedVendor(Path javaHome, Path jdksDirectory) {
    Path install = javaHome.toAbsolutePath().normalize();
    while (install.getParent() != null
        && !install
            .getParent()
            .toAbsolutePath()
            .normalize()
            .equals(jdksDirectory.toAbsolutePath().normalize())) {
      install = install.getParent();
    }
    String name = install.getFileName() == null ? "" : install.getFileName().toString();
    for (String vendor : List.of("adoptium", "corretto", "zulu", "oracle", "graalvm")) {
      if (name.startsWith(vendor + "-")) return vendor;
    }
    return "unknown";
  }

  public static Path tool(Path javaHome, String name) {
    Path tool = javaHome.resolve("bin").resolve(Platform.executableName(name));
    if (!Files.isExecutable(tool)) {
      throw new IllegalStateException("JDK tool is missing or not executable: " + tool);
    }
    return tool;
  }

  public record ToolchainRequest(String version, String vendor) {
    public String specification() {
      return vendor == null ? version : vendor + "-" + version;
    }
  }

  public record Installation(int feature, String version, Path home, String vendor) {
    public Installation(int feature, String version, Path home) {
      this(feature, version, home, "unknown");
    }
  }
}
