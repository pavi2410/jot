package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class JavaToolchainManagerTest {
  @TempDir Path temp;

  @Test
  void installsAndReusesAChecksummedJdkArchive() throws Exception {
    byte[] archive = jdkArchive();
    String checksum =
        HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(archive));
    AtomicInteger metadataRequests = new AtomicInteger();
    JavaToolchainManager.Transport transport =
        new JavaToolchainManager.Transport() {
          @Override
          public String get(URI uri) {
            metadataRequests.incrementAndGet();
            return assetJson(checksum);
          }

          @Override
          public void download(URI uri, Path destination) throws IOException {
            Files.write(destination, archive);
          }
        };
    JavaToolchainManager manager =
        new JavaToolchainManager(
            temp.resolve("jot"), URI.create("https://api.example/v3/"), transport);

    JavaToolchain.Installation installed = manager.install("21", false, false);

    assertEquals(21, installed.feature());
    assertEquals("21.0.2", installed.version());
    assertTrue(Files.isRegularFile(installed.home().resolve("bin/java")));
    assertTrue(Files.isRegularFile(installed.home().resolve("bin/javac")));
    assertTrue(Files.readString(installed.home().resolve("release")).contains("21.0.2"));
    assertTrue(
        Files.readString(installed.home().getParent().resolve("install.json"))
            .contains("\"vendor\":\"adoptium\""));
    assertEquals(List.of(installed), JavaToolchain.installedJdks(temp.resolve("jot/jdks")));

    JavaToolchain.Installation reused = manager.install("21", false, true);

    assertEquals(installed, reused);
    assertEquals(1, metadataRequests.get());
  }

  @Test
  void installsTheRequestedJdkVendorFromTheMatchingAdoptiumDistribution() throws Exception {
    byte[] archive = jdkArchive();
    String checksum =
        HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(archive));
    AtomicReference<URI> metadataUri = new AtomicReference<>();
    JavaToolchainManager.Transport transport =
        new JavaToolchainManager.Transport() {
          @Override
          public String get(URI uri) {
            metadataUri.set(uri);
            return assetJson(checksum);
          }

          @Override
          public void download(URI uri, Path destination) throws IOException {
            Files.write(destination, archive);
          }
        };
    JavaToolchainManager manager =
        new JavaToolchainManager(
            temp.resolve("corretto-jot"), URI.create("https://api.example/v3/"), transport);

    JavaToolchain.Installation installed = manager.install("corretto-21", false, false);

    assertEquals("corretto", installed.vendor());
    assertTrue(metadataUri.get().getQuery().contains("vendor=amazon"));
    assertTrue(installed.home().toString().contains("/jdks/corretto-"));
    assertTrue(
        Files.readString(installed.home().getParent().resolve("install.json"))
            .contains("\"vendor\":\"corretto\""));
    assertEquals(
        "corretto",
        JavaToolchain.installedJdks(temp.resolve("corretto-jot/jdks")).getFirst().vendor());
  }

  @Test
  void rejectsZipSlipEntriesDuringJdkExtraction() throws Exception {
    Path archive = temp.resolve("unsafe.zip");
    try (ZipOutputStream zip = new ZipOutputStream(Files.newOutputStream(archive))) {
      zip.putNextEntry(new ZipEntry("../escaped"));
      zip.write("no".getBytes());
      zip.closeEntry();
    }

    IOException error =
        assertThrows(
            IOException.class,
            () -> JavaToolchainManager.extract(archive, temp.resolve("extracted")));

    assertTrue(error.getMessage().contains("escapes installation directory"));
    assertTrue(!Files.exists(temp.resolve("escaped")));
  }

  @Test
  void extractsUnixTarGzJdkArchives() throws Exception {
    Assumptions.assumeFalse(System.getProperty("os.name").toLowerCase().contains("win"));
    Path source = temp.resolve("source/jdk-21");
    Files.createDirectories(source.resolve("bin"));
    Files.writeString(source.resolve("bin/java"), "java");
    Files.writeString(source.resolve("bin/javac"), "javac");
    Files.writeString(source.resolve("release"), "JAVA_VERSION=\"21.0.2\"\n");
    Path archive = temp.resolve("jdk.tar.gz");
    Process tar =
        new ProcessBuilder(
                "tar",
                "-czf",
                archive.toString(),
                "-C",
                temp.resolve("source").toString(),
                "jdk-21")
            .redirectErrorStream(true)
            .start();
    String output = new String(tar.getInputStream().readAllBytes());
    assertEquals(0, tar.waitFor(), output);

    Path destination = temp.resolve("extracted");
    Files.createDirectories(destination);
    JavaToolchainManager.extract(archive, destination);

    assertTrue(Files.isRegularFile(destination.resolve("jdk-21/bin/javac")));
    assertTrue(Files.readString(destination.resolve("jdk-21/release")).contains("21.0.2"));
  }

  @Test
  void rejectsJdkArchivesWithInvalidChecksums() throws Exception {
    byte[] archive = jdkArchive();
    JavaToolchainManager.Transport transport =
        new JavaToolchainManager.Transport() {
          @Override
          public String get(URI uri) {
            return assetJson("0".repeat(64));
          }

          @Override
          public void download(URI uri, Path destination) throws IOException {
            Files.write(destination, archive);
          }
        };
    JavaToolchainManager manager =
        new JavaToolchainManager(
            temp.resolve("bad-jot"), URI.create("https://api.example/v3/"), transport);

    IOException error = assertThrows(IOException.class, () -> manager.install("21", false, false));

    assertTrue(error.getMessage().contains("SHA-256 mismatch"));
    assertEquals(List.of(), JavaToolchain.installedJdks(temp.resolve("bad-jot/jdks")));
  }

  @Test
  void rejectsUnsafeReleaseNamesFromAdoptiumMetadata() {
    IOException error =
        assertThrows(
            IOException.class,
            () ->
                JavaToolchainManager.parseAsset(
                    assetJson("0".repeat(64)).replace("jdk-21.0.2+13", "../escape")));

    assertTrue(error.getMessage().contains("invalid JDK release name"));
  }

  private static String assetJson(String checksum) {
    return """
    [{
      "version": {"semver": "21.0.2+13"},
      "release_name": "jdk-21.0.2+13",
      "binary": {"package": {
        "name": "jdk.zip",
        "link": "https://downloads.example/jdk.zip",
        "checksum": "%s"
      }}
    }]
    """
        .formatted(checksum);
  }

  private static byte[] jdkArchive() throws Exception {
    ByteArrayOutputStream bytes = new ByteArrayOutputStream();
    try (ZipOutputStream zip = new ZipOutputStream(bytes)) {
      add(zip, "jdk-21/bin/java", "java");
      add(zip, "jdk-21/bin/javac", "javac");
      add(zip, "jdk-21/release", "JAVA_VERSION=\"21.0.2\"\n");
    }
    return bytes.toByteArray();
  }

  private static void add(ZipOutputStream zip, String name, String contents) throws IOException {
    zip.putNextEntry(new ZipEntry(name));
    zip.write(contents.getBytes());
    zip.closeEntry();
  }
}
