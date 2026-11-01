package dev.jot.core;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Locale;

/** Shared operating-system, architecture, and JDK-layout helpers. */
public record Platform(OperatingSystem os, Architecture architecture) {
  public enum OperatingSystem {
    MACOS("macos", "mac", "Mac"),
    WINDOWS("windows", "windows", "Windows"),
    LINUX("linux", "linux", "Linux");

    private final String id;
    private final String adoptiumName;
    private final String rustName;

    OperatingSystem(String id, String adoptiumName, String rustName) {
      this.id = id;
      this.adoptiumName = adoptiumName;
      this.rustName = rustName;
    }
  }

  public enum Architecture {
    AARCH64("aarch64", "Aarch64"),
    X64("x64", "X64");

    private final String id;
    private final String rustName;

    Architecture(String id, String rustName) {
      this.id = id;
      this.rustName = rustName;
    }
  }

  public static Platform current() {
    return of(System.getProperty("os.name"), System.getProperty("os.arch"));
  }

  static Platform of(String osName, String architectureName) {
    String normalizedOs = osName.toLowerCase(Locale.ROOT);
    OperatingSystem os;
    if (normalizedOs.contains("mac") || normalizedOs.contains("darwin")) os = OperatingSystem.MACOS;
    else if (normalizedOs.contains("win")) os = OperatingSystem.WINDOWS;
    else if (normalizedOs.contains("linux")) os = OperatingSystem.LINUX;
    else throw new IllegalStateException("unsupported operating system: " + osName);

    String normalizedArchitecture = architectureName.toLowerCase(Locale.ROOT);
    Architecture architecture =
        switch (normalizedArchitecture) {
          case "aarch64", "arm64" -> Architecture.AARCH64;
          case "x86_64", "amd64" -> Architecture.X64;
          default ->
              throw new IllegalStateException("unsupported architecture: " + architectureName);
        };
    return new Platform(os, architecture);
  }

  public String id() {
    return os.id + "-" + architecture.id;
  }

  public String adoptiumOs() {
    return os.adoptiumName;
  }

  public String adoptiumArchitecture() {
    return architecture.id;
  }

  public String rustOs() {
    return os.rustName;
  }

  public String rustArchitecture() {
    return architecture.rustName;
  }

  public boolean isWindows() {
    return os == OperatingSystem.WINDOWS;
  }

  public static boolean isWindows(String osName) {
    return osName.toLowerCase(Locale.ROOT).startsWith("windows");
  }

  public static String executableName(String name) {
    return executableName(name, System.getProperty("os.name"));
  }

  public static String executableName(String name, String osName) {
    return isWindows(osName) ? name + ".exe" : name;
  }

  public static boolean isJdkHome(Path home) {
    return isJdkHome(home, System.getProperty("os.name"));
  }

  static boolean isJdkHome(Path home, String osName) {
    return Files.isRegularFile(home.resolve("bin").resolve(executableName("java", osName)))
        && Files.isRegularFile(home.resolve("bin").resolve(executableName("javac", osName)));
  }
}
