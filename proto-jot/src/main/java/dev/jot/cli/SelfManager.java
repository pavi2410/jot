package dev.jot.cli;

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
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.regex.Pattern;

/** Locates and manages the currently running universal Jot JAR. */
final class SelfManager {
  private static final ObjectMapper JSON = new ObjectMapper();
  private static final Pattern VERSION =
      Pattern.compile("v?[0-9]+\\.[0-9]+\\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?");
  private static final String REPOSITORY =
      System.getenv().getOrDefault("JOT_RELEASE_REPO", "pavi2410/jot");
  private static final HttpClient HTTP =
      HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NORMAL).build();

  private SelfManager() {}

  static void execute(CommandLine.Self command, String currentVersion) throws Exception {
    switch (command.action()) {
      case CommandLine.SelfUpdate update -> update(update, currentVersion);
      case CommandLine.SelfUninstall uninstall -> uninstall(uninstall);
    }
  }

  private static void update(CommandLine.SelfUpdate command, String currentVersion)
      throws Exception {
    Release release = release(command.version());
    if (command.check()) {
      System.out.println("current: " + currentVersion);
      System.out.println("latest:  " + release.version());
      System.out.println(
          normalizeVersion(release.version()).equals(normalizeVersion(currentVersion))
              ? "jot is up to date"
              : "update available");
      return;
    }
    Path runningJar = runningJar();
    if (normalizeVersion(release.version()).equals(normalizeVersion(currentVersion))) {
      System.out.println("jot is already up to date (" + currentVersion + ")");
      return;
    }
    confirm(command.yes(), "Update jot from " + currentVersion + " to " + release.version() + "?");
    byte[] jar = get(release.jarUrl());
    String expected =
        checksum(
            new String(get(release.checksumsUrl()), java.nio.charset.StandardCharsets.UTF_8),
            release.jarName());
    String actual = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(jar));
    if (!actual.equalsIgnoreCase(expected))
      throw new IOException("downloaded Jot JAR failed SHA-256 verification");
    Path temporary = Files.createTempFile(runningJar.getParent(), ".jot-update-", ".jar");
    try {
      Files.write(temporary, jar);
      try {
        Files.move(
            temporary,
            runningJar,
            StandardCopyOption.REPLACE_EXISTING,
            StandardCopyOption.ATOMIC_MOVE);
      } catch (AtomicMoveNotSupportedException ignored) {
        Files.move(temporary, runningJar, StandardCopyOption.REPLACE_EXISTING);
      }
    } finally {
      Files.deleteIfExists(temporary);
    }
    System.out.println("updated jot to " + release.version());
  }

  private static String normalizeVersion(String version) {
    return version.startsWith("v") ? version.substring(1) : version;
  }

  private static void uninstall(CommandLine.SelfUninstall command) throws Exception {
    Path jar = runningJar();
    confirm(command.yes(), "Uninstall Jot by removing " + jar + "?");
    Files.delete(jar);
    System.out.println("uninstalled jot (removed " + jar + ")");
  }

  private static void confirm(boolean yes, String prompt) throws IOException {
    if (yes) return;
    if (System.console() == null)
      throw new IOException("confirmation requires an interactive terminal; pass --yes");
    if (!System.console().readLine("%s [y/N] ", prompt).equalsIgnoreCase("y"))
      throw new IOException("cancelled");
  }

  private static Path runningJar() throws Exception {
    URI location = Main.class.getProtectionDomain().getCodeSource().getLocation().toURI();
    Path path = Path.of(location).toAbsolutePath().normalize();
    if (!Files.isRegularFile(path) || !path.getFileName().toString().endsWith(".jar"))
      throw new IOException("self update requires running Jot from its packaged JAR");
    return path;
  }

  static Path runningJarIfPackaged() {
    try {
      URI location = Main.class.getProtectionDomain().getCodeSource().getLocation().toURI();
      Path path = Path.of(location).toAbsolutePath().normalize();
      return Files.isRegularFile(path) && path.getFileName().toString().endsWith(".jar")
          ? path
          : null;
    } catch (Exception ignored) {
      return null;
    }
  }

  private static Release release(String requestedVersion) throws Exception {
    String endpoint;
    if (requestedVersion == null)
      endpoint = "https://api.github.com/repos/" + REPOSITORY + "/releases/latest";
    else {
      String tag = requestedVersion.startsWith("v") ? requestedVersion : "v" + requestedVersion;
      if (!VERSION.matcher(tag).matches())
        throw new IllegalArgumentException("invalid release version: " + requestedVersion);
      endpoint = "https://api.github.com/repos/" + REPOSITORY + "/releases/tags/" + tag;
    }
    JsonNode json = JSON.readTree(get(endpoint));
    String version = json.path("tag_name").asText();
    String jarName = "jot-java-" + version + ".jar";
    String jarUrl = null;
    String sumsUrl = null;
    for (JsonNode asset : json.path("assets")) {
      String name = asset.path("name").asText();
      if (name.equals(jarName)) jarUrl = asset.path("browser_download_url").asText();
      if (name.equals("SHA256SUMS")) sumsUrl = asset.path("browser_download_url").asText();
    }
    if (jarUrl == null || sumsUrl == null)
      throw new IOException("release " + version + " does not contain the Java JAR and SHA256SUMS");
    return new Release(version, jarName, jarUrl, sumsUrl);
  }

  private static String checksum(String sums, String filename) throws IOException {
    for (String line : sums.split("\\R")) {
      String[] fields = line.trim().split("\\s+", 2);
      if (fields.length == 2 && fields[1].replaceFirst("^\\*", "").equals(filename))
        return fields[0];
    }
    throw new IOException("SHA256SUMS does not contain " + filename);
  }

  private static byte[] get(String url) throws Exception {
    HttpRequest request =
        HttpRequest.newBuilder(URI.create(url)).header("User-Agent", "jot-java").GET().build();
    HttpResponse<byte[]> response = HTTP.send(request, HttpResponse.BodyHandlers.ofByteArray());
    if (response.statusCode() < 200 || response.statusCode() >= 300)
      throw new IOException("request failed with HTTP " + response.statusCode() + ": " + url);
    return response.body();
  }

  private record Release(String version, String jarName, String jarUrl, String checksumsUrl) {}
}
