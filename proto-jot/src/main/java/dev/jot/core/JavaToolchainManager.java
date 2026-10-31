package dev.jot.core;

import com.fasterxml.jackson.core.JsonFactory;
import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.OffsetDateTime;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.List;
import java.util.Locale;
import java.util.Properties;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/** Installs JDK distributions from Adoptium's API into Jot's shared toolchain directory. */
public final class JavaToolchainManager {
  private static final URI ADOPTIUM_API = URI.create("https://api.adoptium.net/v3/");
  private static final JsonFactory JSON = new JsonFactory();
  private final Path jotHome;
  private final URI apiBase;
  private final Transport transport;

  public JavaToolchainManager() {
    this(Path.of(System.getProperty("user.home"), ".jot"));
  }

  JavaToolchainManager(Path jotHome) {
    this(jotHome, ADOPTIUM_API);
  }

  JavaToolchainManager(Path jotHome, URI apiBase) {
    this.jotHome = jotHome.toAbsolutePath().normalize();
    this.apiBase = apiBase;
    this.transport =
        new HttpTransport(
            HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(30))
                .followRedirects(HttpClient.Redirect.NORMAL)
                .build());
  }

  JavaToolchainManager(Path jotHome, URI apiBase, Transport transport) {
    this.jotHome = jotHome.toAbsolutePath().normalize();
    this.apiBase = apiBase;
    this.transport = transport;
  }

  public JavaToolchain.Installation install(String requestedVersion, boolean force, boolean offline)
      throws Exception {
    JavaToolchain.ToolchainRequest request = JavaToolchain.parseRequest(requestedVersion);
    String version = request.version();
    String vendor = request.vendor() == null ? "adoptium" : request.vendor();
    String apiVendor =
        switch (vendor) {
          case "adoptium" -> "eclipse";
          case "corretto" -> "amazon";
          case "zulu" -> "azul";
          case "oracle" -> "oracle";
          default -> throw new IllegalArgumentException("unsupported JDK vendor: " + vendor);
        };
    int feature = JavaToolchain.parseRelease(version);
    Path jdks = jotHome.resolve("jdks");
    Path downloads = jotHome.resolve("downloads");
    Path locks = jotHome.resolve("locks");
    Files.createDirectories(jdks);
    Files.createDirectories(downloads);
    Files.createDirectories(locks);

    Path lockPath = locks.resolve("jdk-" + vendor + "-" + version + "-" + platform() + ".lock");
    try (FileChannel channel =
            FileChannel.open(lockPath, StandardOpenOption.CREATE, StandardOpenOption.WRITE);
        FileLock ignored = channel.lock()) {
      List<JavaToolchain.Installation> installed = JavaToolchain.installedJdks(jdks);
      if (!force) {
        for (JavaToolchain.Installation installation : installed) {
          if (installation.feature() == feature && installation.vendor().equals(vendor))
            return installation;
        }
      }
      if (offline) {
        throw new IOException(
            "offline mode: cannot install Java "
                + request.specification()
                + " because it is not already installed");
      }

      Asset asset = resolveAsset(version, apiVendor);
      Path vendorDownloads = downloads.resolve(vendor);
      Files.createDirectories(vendorDownloads);
      Path archive = vendorDownloads.resolve(asset.name());
      fetchArchive(asset, archive);
      Path installationDirectory =
          jdks.resolve(vendor + "-" + asset.releaseName() + "-" + platform());
      if (Files.exists(installationDirectory)) deleteTree(installationDirectory);
      Path staging = Files.createTempDirectory(jdks, ".jdk-install-");
      try {
        extract(archive, staging);
        Path extractedHome = findJavaHome(staging);
        try {
          Files.move(staging, installationDirectory, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignoredMove) {
          Files.move(staging, installationDirectory);
        }
        Path javaHome = installationDirectory.resolve(staging.relativize(extractedHome));
        String installedVersion = readJavaVersion(javaHome);
        JavaToolchain.Installation result =
            new JavaToolchain.Installation(feature, installedVersion, javaHome, vendor);
        writeRustCompatibleMetadata(
            installationDirectory.resolve("install.json"),
            vendor,
            version,
            asset,
            javaHome,
            installationDirectory);
        return result;
      } catch (Exception failure) {
        if (Files.exists(staging)) deleteTree(staging);
        throw failure;
      }
    }
  }

  static Asset parseAsset(String json) throws IOException {
    try (JsonParser parser = JSON.createParser(json)) {
      if (parser.nextToken() != JsonToken.START_ARRAY) {
        throw new IOException("Adoptium returned an invalid asset list");
      }
      while (parser.nextToken() != JsonToken.END_ARRAY) {
        if (parser.currentToken() != JsonToken.START_OBJECT) {
          parser.skipChildren();
          continue;
        }
        Asset asset = readAsset(parser);
        if (asset != null) return asset;
      }
    }
    throw new IOException("Adoptium returned no matching JDK asset");
  }

  static void extract(Path archive, Path destination) throws Exception {
    String fileName = archive.getFileName().toString();
    if (fileName.endsWith(".zip")) {
      try (ZipInputStream input = new ZipInputStream(Files.newInputStream(archive))) {
        ZipEntry entry;
        while ((entry = input.getNextEntry()) != null) {
          Path output = destination.resolve(entry.getName()).normalize();
          if (!output.startsWith(destination)) {
            throw new IOException(
                "archive entry escapes installation directory: " + entry.getName());
          }
          if (entry.isDirectory()) {
            Files.createDirectories(output);
          } else {
            Files.createDirectories(output.getParent());
            Files.copy(input, output, StandardCopyOption.REPLACE_EXISTING);
          }
          input.closeEntry();
        }
      }
      return;
    }
    if (fileName.endsWith(".tar.gz") || fileName.endsWith(".tgz")) {
      Process process =
          new ProcessBuilder("tar", "-xzf", archive.toString(), "-C", destination.toString())
              .redirectErrorStream(true)
              .start();
      String output = new String(process.getInputStream().readAllBytes());
      int status = process.waitFor();
      if (status != 0) {
        throw new IOException("could not extract JDK archive (tar exit " + status + "): " + output);
      }
      return;
    }
    throw new IOException("unsupported JDK archive format: " + archive);
  }

  private Asset resolveAsset(String version, String vendor) throws Exception {
    String os = adoptiumOs();
    String architecture = adoptiumArchitecture();
    URI uri =
        apiBase.resolve(
            "assets/latest/"
                + version
                + "/hotspot?release_type=ga&os="
                + os
                + "&architecture="
                + architecture
                + "&image_type=jdk&vendor="
                + vendor);
    return parseAsset(transport.get(uri));
  }

  private void fetchArchive(Asset asset, Path destination) throws Exception {
    if (Files.isRegularFile(destination)) {
      try {
        verifyChecksum(destination, asset.checksum());
        return;
      } catch (IOException invalidCache) {
        Files.delete(destination);
      }
    }
    Path temporary = Files.createTempFile(destination.getParent(), ".jdk-download-", ".tmp");
    try {
      transport.download(URI.create(asset.link()), temporary);
      verifyChecksum(temporary, asset.checksum());
      try {
        Files.move(temporary, destination, StandardCopyOption.ATOMIC_MOVE);
      } catch (AtomicMoveNotSupportedException ignored) {
        Files.move(temporary, destination, StandardCopyOption.REPLACE_EXISTING);
      }
    } finally {
      Files.deleteIfExists(temporary);
    }
  }

  private static Asset readAsset(JsonParser parser) throws IOException {
    String releaseName = null;
    String semver = null;
    Package assetPackage = null;
    while (parser.nextToken() != JsonToken.END_OBJECT) {
      if (parser.currentToken() != JsonToken.FIELD_NAME) continue;
      String field = parser.currentName();
      JsonToken value = parser.nextToken();
      if (field.equals("release_name")) releaseName = parser.getValueAsString();
      else if (field.equals("version") && value == JsonToken.START_OBJECT)
        semver = readSemver(parser);
      else if (field.equals("binary") && value == JsonToken.START_OBJECT) {
        assetPackage = readBinary(parser);
      } else parser.skipChildren();
    }
    if (releaseName == null || semver == null || assetPackage == null) return null;
    if (releaseName.contains("/") || releaseName.contains("\\") || releaseName.equals("..")) {
      throw new IOException("invalid JDK release name in Adoptium metadata: " + releaseName);
    }
    return new Asset(
        releaseName, semver, assetPackage.name(), assetPackage.link(), assetPackage.checksum());
  }

  private static String readSemver(JsonParser parser) throws IOException {
    String semver = null;
    while (parser.nextToken() != JsonToken.END_OBJECT) {
      if (parser.currentToken() == JsonToken.FIELD_NAME) {
        String field = parser.currentName();
        JsonToken value = parser.nextToken();
        if (field.equals("semver")) semver = parser.getValueAsString();
        else parser.skipChildren();
      }
    }
    return semver;
  }

  private static Package readBinary(JsonParser parser) throws IOException {
    Package result = null;
    while (parser.nextToken() != JsonToken.END_OBJECT) {
      if (parser.currentToken() == JsonToken.FIELD_NAME) {
        String field = parser.currentName();
        JsonToken value = parser.nextToken();
        if (field.equals("package") && value == JsonToken.START_OBJECT)
          result = readPackage(parser);
        else parser.skipChildren();
      }
    }
    return result;
  }

  private static Package readPackage(JsonParser parser) throws IOException {
    String name = null;
    String link = null;
    String checksum = null;
    while (parser.nextToken() != JsonToken.END_OBJECT) {
      if (parser.currentToken() != JsonToken.FIELD_NAME) continue;
      String field = parser.currentName();
      parser.nextToken();
      switch (field) {
        case "name" -> name = parser.getValueAsString();
        case "link" -> link = parser.getValueAsString();
        case "checksum" -> checksum = parser.getValueAsString();
        default -> parser.skipChildren();
      }
    }
    if (name == null || link == null || checksum == null) return null;
    Path fileName = Path.of(name).getFileName();
    if (fileName == null || !fileName.toString().equals(name)) {
      throw new IOException("invalid JDK archive filename in Adoptium metadata: " + name);
    }
    return new Package(name, link, checksum);
  }

  private static Path findJavaHome(Path root) throws IOException {
    try (Stream<Path> files = Files.walk(root, 8)) {
      return files
          .filter(path -> path.getFileName().toString().equals("release"))
          .filter(path -> Files.isRegularFile(path.resolveSibling("bin/java")))
          .filter(path -> Files.isRegularFile(path.resolveSibling("bin/javac")))
          .map(Path::getParent)
          .findFirst()
          .orElseThrow(
              () -> new IOException("could not detect JAVA_HOME in extracted JDK archive"));
    }
  }

  private static String readJavaVersion(Path javaHome) throws IOException {
    Properties release = new Properties();
    try (var input = Files.newInputStream(javaHome.resolve("release"))) {
      release.load(input);
    }
    String version = release.getProperty("JAVA_VERSION");
    if (version == null) throw new IOException("JDK release file has no JAVA_VERSION field");
    if (version.length() >= 2 && version.startsWith("\"") && version.endsWith("\"")) {
      return version.substring(1, version.length() - 1);
    }
    return version;
  }

  private static void writeRustCompatibleMetadata(
      Path file,
      String vendor,
      String requestedVersion,
      Asset asset,
      Path javaHome,
      Path installDirectory)
      throws IOException {
    OffsetDateTime now = OffsetDateTime.now();
    try (JsonGenerator json = JSON.createGenerator(Files.newOutputStream(file))) {
      json.writeStartObject();
      json.writeStringField("vendor", vendor);
      json.writeStringField("requested_version", requestedVersion);
      json.writeStringField("release_name", asset.releaseName());
      json.writeStringField("semver", asset.semver());
      json.writeStringField("java_home", javaHome.toString());
      json.writeStringField("install_dir", installDirectory.toString());
      json.writeObjectFieldStart("platform");
      json.writeStringField("os", rustOs());
      json.writeStringField("arch", rustArchitecture());
      json.writeEndObject();
      json.writeArrayFieldStart("installed_at");
      json.writeNumber(now.getYear());
      json.writeNumber(now.getDayOfYear());
      json.writeNumber(now.getHour());
      json.writeNumber(now.getMinute());
      json.writeNumber(now.getSecond());
      json.writeNumber(now.getNano());
      json.writeNumber(now.getOffset().getTotalSeconds() / 3600);
      json.writeNumber((now.getOffset().getTotalSeconds() % 3600) / 60);
      json.writeNumber(now.getOffset().getTotalSeconds() % 60);
      json.writeEndArray();
      json.writeEndObject();
    }
  }

  private static void verifyChecksum(Path file, String expected) throws Exception {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    try (var input = Files.newInputStream(file)) {
      byte[] buffer = new byte[64 * 1024];
      int read;
      while ((read = input.read(buffer)) >= 0) digest.update(buffer, 0, read);
    }
    String actual = HexFormat.of().formatHex(digest.digest());
    if (!actual.equalsIgnoreCase(expected)) {
      throw new IOException(
          "JDK archive SHA-256 mismatch: expected " + expected + ", got " + actual);
    }
  }

  private static void deleteTree(Path root) throws IOException {
    try (Stream<Path> paths = Files.walk(root)) {
      for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
    }
  }

  private static String platform() {
    String operatingSystem = System.getProperty("os.name").toLowerCase(Locale.ROOT);
    String os;
    if (operatingSystem.contains("mac") || operatingSystem.contains("darwin")) os = "macos";
    else if (operatingSystem.contains("win")) os = "windows";
    else if (operatingSystem.contains("linux")) os = "linux";
    else throw new IllegalStateException("unsupported JDK operating system: " + operatingSystem);
    return os + "-" + (adoptiumArchitecture().equals("aarch64") ? "aarch64" : "x64");
  }

  private static String adoptiumOs() {
    return platform().startsWith("macos")
        ? "mac"
        : platform().substring(0, platform().indexOf('-'));
  }

  private static String adoptiumArchitecture() {
    return switch (System.getProperty("os.arch").toLowerCase(Locale.ROOT)) {
      case "aarch64", "arm64" -> "aarch64";
      case "x86_64", "amd64" -> "x64";
      default ->
          throw new IllegalStateException(
              "unsupported JDK architecture: " + System.getProperty("os.arch"));
    };
  }

  private static String rustOs() {
    return switch (platform().substring(0, platform().indexOf('-'))) {
      case "macos" -> "Mac";
      case "windows" -> "Windows";
      default -> "Linux";
    };
  }

  private static String rustArchitecture() {
    return platform().endsWith("aarch64") ? "Aarch64" : "X64";
  }

  private record Package(String name, String link, String checksum) {}

  record Asset(String releaseName, String semver, String name, String link, String checksum) {}

  interface Transport {
    String get(URI uri) throws Exception;

    void download(URI uri, Path destination) throws Exception;
  }

  private static final class HttpTransport implements Transport {
    private final HttpClient client;

    private HttpTransport(HttpClient client) {
      this.client = client;
    }

    @Override
    public String get(URI uri) throws Exception {
      HttpResponse<String> response =
          client.send(
              HttpRequest.newBuilder(uri).timeout(Duration.ofMinutes(2)).GET().build(),
              HttpResponse.BodyHandlers.ofString());
      if (response.statusCode() < 200 || response.statusCode() >= 300) {
        throw new IOException("Adoptium returned HTTP " + response.statusCode() + " for " + uri);
      }
      return response.body();
    }

    @Override
    public void download(URI uri, Path destination) throws Exception {
      HttpResponse<Path> response =
          client.send(
              HttpRequest.newBuilder(uri).timeout(Duration.ofMinutes(30)).GET().build(),
              HttpResponse.BodyHandlers.ofFile(destination));
      if (response.statusCode() < 200 || response.statusCode() >= 300) {
        throw new IOException(
            "JDK download returned HTTP " + response.statusCode() + " for " + uri);
      }
    }
  }
}
