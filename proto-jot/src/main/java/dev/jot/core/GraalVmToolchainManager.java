package dev.jot.core;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.List;
import java.util.Locale;
import java.util.Properties;
import java.util.stream.Stream;

/** Finds and installs GraalVM Community JDKs with Native Image support. */
public final class GraalVmToolchainManager {
  private static final URI RELEASES_API =
      URI.create("https://api.github.com/repos/graalvm/graalvm-ce-builds/releases");
  private static final ObjectMapper JSON = new ObjectMapper();
  private final Path jotHome;
  private final Transport transport;

  public GraalVmToolchainManager() {
    this(Path.of(System.getProperty("user.home"), ".jot"));
  }

  GraalVmToolchainManager(Path jotHome) {
    this(jotHome, new HttpTransport());
  }

  GraalVmToolchainManager(Path jotHome, Transport transport) {
    this.jotHome = jotHome.toAbsolutePath().normalize();
    this.transport = transport;
  }

  public JavaToolchain.Installation install(String requestedFeature, boolean force, boolean offline)
      throws Exception {
    int feature = JavaToolchain.parseRelease(requestedFeature);
    Path jdks = jotHome.resolve("jdks");
    Files.createDirectories(jdks);
    List<JavaToolchain.Installation> installed = JavaToolchain.installedJdks(jdks);
    if (!force) {
      for (JavaToolchain.Installation installation : installed) {
        if (installation.feature() == feature && installation.vendor().equals("graalvm")) {
          return installation;
        }
      }
    }
    if (offline) {
      throw new IOException(
          "offline mode: GraalVM Java "
              + feature
              + " is not installed; install it with `jot toolchain install graalvm@"
              + feature
              + "` online first");
    }

    Path locks = jotHome.resolve("locks");
    Path downloads = jotHome.resolve("downloads/graalvm");
    Files.createDirectories(locks);
    Files.createDirectories(downloads);
    Path lockPath = locks.resolve("graalvm-" + feature + "-" + platform() + ".lock");
    try (var channel =
            java.nio.channels.FileChannel.open(
                lockPath, StandardOpenOption.CREATE, StandardOpenOption.WRITE);
        var ignored = channel.lock()) {
      ReleaseAsset asset = resolveAsset(feature, platform());
      Path archive = downloads.resolve(asset.name());
      if (!Files.isRegularFile(archive) || !checksum(archive).equalsIgnoreCase(asset.sha256())) {
        Files.deleteIfExists(archive);
        transport.download(URI.create(asset.url()), archive);
      }
      String actualChecksum = checksum(archive);
      if (!actualChecksum.equalsIgnoreCase(asset.sha256())) {
        Files.deleteIfExists(archive);
        throw new IOException(
            "GraalVM archive SHA-256 mismatch: expected "
                + asset.sha256()
                + ", got "
                + actualChecksum);
      }

      Path installDirectory = jdks.resolve("graalvm-jdk-" + feature + "-" + platform());
      Path staging = Files.createTempDirectory(jdks, ".graalvm-install-");
      try {
        JavaToolchainManager.extract(archive, staging);
        Path extractedHome = findGraalVmHome(staging);
        Path relativeHome = staging.relativize(extractedHome);
        if (Files.exists(installDirectory)) deleteTree(installDirectory);
        try {
          Files.move(staging, installDirectory, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignoredMove) {
          Files.move(staging, installDirectory);
        }
        Path home = installDirectory.resolve(relativeHome);
        String version = readJavaVersion(home);
        return new JavaToolchain.Installation(feature, version, home, "graalvm");
      } catch (Exception failure) {
        if (Files.exists(staging)) deleteTree(staging);
        throw failure;
      }
    }
  }

  public static Path installedHome(String requestedFeature) throws IOException {
    int feature = JavaToolchain.parseRelease(requestedFeature);
    Path jdks = Path.of(System.getProperty("user.home"), ".jot", "jdks");
    for (JavaToolchain.Installation installation : JavaToolchain.installedJdks(jdks)) {
      if (installation.feature() == feature && installation.vendor().equals("graalvm")) {
        Path nativeImage = nativeImageTool(installation.home());
        if (Files.isRegularFile(nativeImage)) return installation.home();
      }
    }
    throw new IOException(
        "GraalVM Java "
            + feature
            + " is not installed; install it with `jot toolchain install graalvm@"
            + feature
            + "`");
  }

  static ReleaseAsset parseAssetList(String json, int requestedFeature, String targetPlatform)
      throws IOException {
    JsonNode releases = JSON.readTree(json);
    if (!releases.isArray())
      throw new IOException("GitHub returned an invalid GraalVM release list");
    for (JsonNode release : releases) {
      if (release.path("draft").asBoolean() || release.path("prerelease").asBoolean()) continue;
      JsonNode assets = release.path("assets");
      for (JsonNode asset : assets) {
        String name = asset.path("name").asText("");
        if (!matches(name, requestedFeature, targetPlatform)) continue;
        String digest = asset.path("digest").asText("");
        if (!digest.startsWith("sha256:")) {
          throw new IOException("GraalVM release asset does not publish a SHA-256 digest: " + name);
        }
        String url = asset.path("browser_download_url").asText("");
        if (url.isBlank())
          throw new IOException("GraalVM release asset has no download URL: " + name);
        return new ReleaseAsset(name, url, digest.substring("sha256:".length()));
      }
    }
    return null;
  }

  private ReleaseAsset resolveAsset(int feature, String targetPlatform) throws Exception {
    for (int page = 1; page <= 5; page++) {
      URI uri = URI.create(RELEASES_API + "?per_page=100&page=" + page);
      String body = transport.get(uri);
      ReleaseAsset asset = parseAssetList(body, feature, targetPlatform);
      if (asset != null) return asset;
      JsonNode releases = JSON.readTree(body);
      if (releases.size() < 100) break;
    }
    throw new IOException(
        "no GraalVM Community release for Java "
            + feature
            + " on "
            + targetPlatform
            + " was found in the official releases");
  }

  private static boolean matches(String name, int feature, String targetPlatform) {
    if (!(name.endsWith(".tar.gz") || name.endsWith(".zip"))) return false;
    if (!name.contains("_" + targetPlatform + "_bin.")) return false;
    java.util.regex.Matcher matcher =
        java.util.regex.Pattern.compile("-jdk-(\\d+)(?:i\\d+)?[.-]").matcher(name);
    return matcher.find() && Integer.parseInt(matcher.group(1)) == feature;
  }

  static String platform() {
    String os = System.getProperty("os.name").toLowerCase(Locale.ROOT);
    String arch = System.getProperty("os.arch").toLowerCase(Locale.ROOT);
    String architecture =
        switch (arch) {
          case "aarch64", "arm64" -> "aarch64";
          case "x86_64", "amd64" -> "x64";
          default -> throw new IllegalStateException("unsupported GraalVM architecture: " + arch);
        };
    if (os.contains("mac")) {
      return "macos-" + architecture;
    }
    if (os.contains("win")) {
      if (!architecture.equals("x64"))
        throw new IllegalStateException("GraalVM Community Windows builds require x64");
      return "windows-x64";
    }
    if (os.contains("linux")) return "linux-" + architecture;
    throw new IllegalStateException("unsupported GraalVM operating system: " + os);
  }

  private static Path findGraalVmHome(Path root) throws IOException {
    try (Stream<Path> files = Files.walk(root, 8)) {
      return files
          .filter(path -> path.getFileName().toString().equals("release"))
          .map(Path::getParent)
          .filter(home -> Files.isRegularFile(nativeImageTool(home)))
          .filter(home -> Files.isRegularFile(home.resolve("bin/java")))
          .filter(home -> Files.isRegularFile(home.resolve("bin/javac")))
          .findFirst()
          .orElseThrow(() -> new IOException("GraalVM archive contains no complete JDK"));
    }
  }

  public static Path nativeImageTool(Path home) {
    return home.resolve("bin").resolve(isWindows() ? "native-image.cmd" : "native-image");
  }

  private static String readJavaVersion(Path home) throws IOException {
    Properties release = new Properties();
    try (var input = Files.newInputStream(home.resolve("release"))) {
      release.load(input);
    }
    String value = release.getProperty("JAVA_VERSION", "");
    return value.length() >= 2 && value.startsWith("\"") && value.endsWith("\"")
        ? value.substring(1, value.length() - 1)
        : value;
  }

  private static String checksum(Path file) throws Exception {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    try (var input = Files.newInputStream(file)) {
      byte[] buffer = new byte[64 * 1024];
      int count;
      while ((count = input.read(buffer)) >= 0) digest.update(buffer, 0, count);
    }
    return HexFormat.of().formatHex(digest.digest());
  }

  private static boolean isWindows() {
    return System.getProperty("os.name").toLowerCase(Locale.ROOT).contains("win");
  }

  private static void deleteTree(Path root) throws IOException {
    if (!Files.exists(root)) return;
    try (Stream<Path> paths = Files.walk(root)) {
      for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
    }
  }

  record ReleaseAsset(String name, String url, String sha256) {}

  interface Transport {
    String get(URI uri) throws Exception;

    void download(URI uri, Path destination) throws Exception;
  }

  private static final class HttpTransport implements Transport {
    private final HttpClient client =
        HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(30))
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build();

    @Override
    public String get(URI uri) throws Exception {
      HttpResponse<String> response =
          client.send(
              HttpRequest.newBuilder(uri)
                  .timeout(Duration.ofMinutes(2))
                  .header("User-Agent", "jot-java-toolchain-manager")
                  .header("Accept", "application/vnd.github+json")
                  .header("X-GitHub-Api-Version", "2022-11-28")
                  .GET()
                  .build(),
              HttpResponse.BodyHandlers.ofString());
      if (response.statusCode() < 200 || response.statusCode() >= 300) {
        throw new IOException("GitHub returned HTTP " + response.statusCode() + " for " + uri);
      }
      return response.body();
    }

    @Override
    public void download(URI uri, Path destination) throws Exception {
      HttpResponse<Path> response =
          client.send(
              HttpRequest.newBuilder(uri)
                  .timeout(Duration.ofMinutes(45))
                  .header("User-Agent", "jot-java-toolchain-manager")
                  .GET()
                  .build(),
              HttpResponse.BodyHandlers.ofFile(destination));
      if (response.statusCode() < 200 || response.statusCode() >= 300) {
        throw new IOException(
            "GraalVM download returned HTTP " + response.statusCode() + " for " + uri);
      }
    }
  }
}
