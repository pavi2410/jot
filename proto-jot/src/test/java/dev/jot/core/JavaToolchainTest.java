package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class JavaToolchainTest {
  @TempDir Path temp;

  @Test
  void parsesOptionalJavaVendorPrefixesAndAliases() {
    assertEquals("21", JavaToolchain.parseRequest("21").version());
    assertNull(JavaToolchain.parseRequest("21").vendor());
    assertEquals("corretto", JavaToolchain.parseRequest("corretto-21").vendor());
    assertEquals("adoptium-21", JavaToolchain.parseRequest("temurin-21").specification());
  }

  @Test
  void selectsAnInstalledMatchingJdkBeforeTheRunningJdk() {
    Path managed = temp.resolve("jdk-21");

    assertEquals(
        managed,
        JavaToolchain.selectHome(
            "21",
            temp.resolve("running-jdk-27"),
            27,
            List.of(new JavaToolchain.Installation(21, "21.0.2", managed))));
  }

  @Test
  void fallsBackToANewerRunningJdkWhenNoExactInstallationExists() {
    Path running = temp.resolve("running-jdk-27");

    assertEquals(running, JavaToolchain.selectHome("21", running, 27, List.of()));
  }

  @Test
  void selectsThePinnedJdkVendorWhenSeveralBuildsMatchTheRelease() {
    Path adoptium = Path.of("/managed/adoptium");
    Path corretto = Path.of("/managed/corretto");

    Path selected =
        JavaToolchain.selectHome(
            "21",
            "corretto",
            Path.of("/running"),
            27,
            "adoptium",
            List.of(
                new JavaToolchain.Installation(21, "21.0.2", adoptium, "adoptium"),
                new JavaToolchain.Installation(21, "21.0.2", corretto, "corretto")));

    assertEquals(corretto, selected);
  }

  @Test
  void explainsHowToInstallARequiredJdkWhenNoCompatibleToolchainExists() {
    IllegalStateException error =
        assertThrows(
            IllegalStateException.class,
            () -> JavaToolchain.selectHome("27", temp.resolve("running-jdk-21"), 21, List.of()));

    assertEquals(
        "project requires Java 27, but Jot runs on Java 21 and no managed JDK 27 is installed;"
            + " install it with `jot toolchain install java@27` using Java Jot",
        error.getMessage());
  }

  @Test
  void discoversJdksFromTheirReleaseFiles() throws IOException {
    Path home = temp.resolve("jdks/vendor-release").resolve("Contents/Home");
    Files.createDirectories(home.resolve("bin"));
    Files.writeString(home.resolve("bin/java"), "java");
    Files.writeString(home.resolve("bin/javac"), "javac");
    Files.writeString(home.resolve("release"), "JAVA_VERSION=\"21.0.2\"\n");

    assertEquals(
        List.of(new JavaToolchain.Installation(21, "21.0.2", home)),
        JavaToolchain.installedJdks(temp.resolve("jdks")));
  }

  @Test
  void rejectsNonNumericProjectReleases() {
    IllegalArgumentException error =
        assertThrows(IllegalArgumentException.class, () -> JavaToolchain.parseRelease("latest"));

    assertEquals("toolchains.java must be a Java release number: latest", error.getMessage());
  }
}
