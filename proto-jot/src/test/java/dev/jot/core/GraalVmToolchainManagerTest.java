package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class GraalVmToolchainManagerTest {
  @TempDir Path temp;

  @Test
  void installsChecksumVerifiedGraalvmAndReusesItOffline() throws Exception {
    Assumptions.assumeFalse(System.getProperty("os.name").toLowerCase().contains("win"));
    String platform = GraalVmToolchainManager.platform();
    Path archive = createArchive(platform);
    byte[] contents = Files.readAllBytes(archive);
    String checksum =
        HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(contents));
    AtomicInteger metadataRequests = new AtomicInteger();
    GraalVmToolchainManager.Transport transport =
        new GraalVmToolchainManager.Transport() {
          @Override
          public String get(URI uri) {
            metadataRequests.incrementAndGet();
            return releaseJson(platform, checksum);
          }

          @Override
          public void download(URI uri, Path destination) throws IOException {
            Files.copy(archive, destination);
          }
        };
    GraalVmToolchainManager manager = new GraalVmToolchainManager(temp.resolve("jot"), transport);

    JavaToolchain.Installation installed = manager.install("25", false, false);

    assertEquals(25, installed.feature());
    assertEquals("graalvm", installed.vendor());
    assertTrue(Files.isRegularFile(installed.home().resolve("bin/native-image")));
    assertEquals(List.of(installed), JavaToolchain.installedJdks(temp.resolve("jot/jdks")));
    assertEquals(installed, manager.install("25", false, true));
    assertEquals(1, metadataRequests.get());
  }

  @Test
  void selectsMatchingGraalvmFeatureAndPlatformAsset() throws Exception {
    String platform = GraalVmToolchainManager.platform();
    String response =
        """
        [{"assets":[
          {"name":"graalvm-community-jdk-21.0.8_%s_bin.tar.gz","digest":"sha256:%s","browser_download_url":"https://example.invalid/old"},
          {"name":"graalvm-community-jdk-25i4-25.0.4.1.1_%s_bin.tar.gz","digest":"sha256:%s","browser_download_url":"https://example.invalid/current"}
        ]}]
        """
            .formatted(platform, "a".repeat(64), platform, "b".repeat(64));

    GraalVmToolchainManager.ReleaseAsset asset =
        GraalVmToolchainManager.parseAssetList(response, 25, platform);

    assertEquals("graalvm-community-jdk-25i4-25.0.4.1.1_" + platform + "_bin.tar.gz", asset.name());
    assertEquals("b".repeat(64), asset.sha256());
  }

  @Test
  void doesNotDownloadUninstalledGraalvmInOfflineMode() {
    GraalVmToolchainManager manager =
        new GraalVmToolchainManager(
            temp.resolve("offline-jot"),
            new GraalVmToolchainManager.Transport() {
              @Override
              public String get(URI uri) {
                throw new AssertionError("offline install must not query GitHub");
              }

              @Override
              public void download(URI uri, Path destination) {
                throw new AssertionError("offline install must not download archives");
              }
            });

    IOException error = assertThrows(IOException.class, () -> manager.install("25", false, true));

    assertTrue(error.getMessage().contains("offline mode: GraalVM Java 25 is not installed"));
  }

  private Path createArchive(String platform) throws Exception {
    Path root = temp.resolve("archive-root");
    Path home = root.resolve("graalvm-community-jdk-25/Contents/Home");
    Files.createDirectories(home.resolve("bin"));
    Files.writeString(home.resolve("bin/java"), "java");
    Files.writeString(home.resolve("bin/javac"), "javac");
    Path nativeImage = home.resolve("bin/native-image");
    Files.writeString(nativeImage, "native-image");
    nativeImage.toFile().setExecutable(true);
    Files.writeString(
        home.resolve("release"),
        "JAVA_VERSION=\"25.0.4.1.1\"\nIMPLEMENTOR=\"GraalVM Community\"\n");
    Path archive = temp.resolve("graalvm-community-jdk-25_" + platform + "_bin.tar.gz");
    Process process =
        new ProcessBuilder(
                "tar",
                "-czf",
                archive.toString(),
                "-C",
                root.toString(),
                "graalvm-community-jdk-25")
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());
    assertEquals(0, process.waitFor(), output);
    return archive;
  }

  private String releaseJson(String platform, String checksum) {
    return """
    [{"assets":[{"name":"graalvm-community-jdk-25i4-25.0.4.1.1_%s_bin.tar.gz","digest":"sha256:%s","browser_download_url":"https://example.invalid/graalvm.tar.gz"}]}]
    """
        .formatted(platform, checksum);
  }
}
