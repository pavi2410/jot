package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class PlatformTest {
  @TempDir Path temp;

  @Test
  void normalizesOperatingSystemAndArchitectureAliasesForToolchainDownloads() {
    Platform macArm = Platform.of("Mac OS X", "arm64");
    Platform linuxX64 = Platform.of("Linux", "AMD64");
    Platform windowsX64 = Platform.of("Windows 11", "x86_64");

    assertEquals("macos-aarch64", macArm.id());
    assertEquals(Platform.OperatingSystem.MACOS, macArm.os());
    assertEquals(Platform.Architecture.AARCH64, macArm.architecture());
    assertEquals("mac", macArm.adoptiumOs());
    assertEquals("Mac", macArm.rustOs());
    assertEquals("Aarch64", macArm.rustArchitecture());
    assertEquals("linux-x64", linuxX64.id());
    assertEquals("windows-x64", windowsX64.id());
    assertEquals("Windows", windowsX64.rustOs());
    assertFalse(Platform.isWindows("Darwin"));
    assertTrue(Platform.isWindows("Windows 11"));
  }

  @Test
  void selectsWindowsExecutableNamesAndDetectsCompleteJdkHomes() throws Exception {
    Path home = temp.resolve("jdk");
    Path bin = Files.createDirectories(home.resolve("bin"));
    Files.writeString(bin.resolve("java.exe"), "java");

    assertEquals("java.exe", Platform.executableName("java", "Windows 11"));
    assertFalse(Platform.isJdkHome(home, "Windows 11"));

    Files.writeString(bin.resolve("javac.exe"), "javac");
    assertTrue(Platform.isJdkHome(home, "Windows 11"));
    assertFalse(Platform.isJdkHome(home, "Linux"));
  }
}
