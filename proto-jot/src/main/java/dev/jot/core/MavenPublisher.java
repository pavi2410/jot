package dev.jot.core;

import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.List;
import java.util.Locale;
import java.util.jar.JarEntry;
import java.util.jar.JarOutputStream;
import java.util.stream.Stream;

/** Stages and publishes Maven artifacts for a built Java project. */
public final class MavenPublisher {
  private static final long REPRODUCIBLE_JAR_TIME = 315_532_800_000L;

  private MavenPublisher() {}

  public static Result publish(
      WorkspaceProject project,
      MavenResolver resolver,
      String repository,
      String username,
      String password,
      boolean allowSnapshot,
      boolean dryRun,
      ArtifactSigner signer)
      throws Exception {
    ProjectConfig config = project.config();
    PublishMetadata metadata = config.publish();
    if (metadata == null)
      throw new IllegalArgumentException("missing [publish] metadata in jot.toml");
    if (!dryRun && (repository == null || repository.isBlank()))
      throw new IllegalArgumentException(
          "missing publish repository; pass --repository or set JOT_PUBLISH_REPOSITORY");
    MavenCoordinate coordinate = coordinate(config, allowSnapshot);
    validateMetadata(metadata);

    BuildEngine.build(project, resolver);
    Path root = project.root();
    Path target = root.resolve("target");
    Path artifact = target.resolve(config.name() + "-" + config.version() + ".jar");
    Path stage = target.resolve("publish").resolve(config.name() + "-" + config.version());
    deleteTree(stage);
    Files.createDirectories(stage);
    List<Path> uploads = new ArrayList<>();
    Path mainJar = stage.resolve(fileName(coordinate, "", "jar"));
    Files.copy(artifact, mainJar);
    uploads.add(mainJar);

    Path sourcesJar = stage.resolve(fileName(coordinate, "-sources", "jar"));
    createSourcesJar(project, sourcesJar);
    uploads.add(sourcesJar);

    Path docs = BuildEngine.doc(project, resolver);
    Path javadocJar = stage.resolve(fileName(coordinate, "-javadoc", "jar"));
    createJar(docs, javadocJar);
    uploads.add(javadocJar);

    Path pom = stage.resolve(fileName(coordinate, "", "pom"));
    Files.writeString(pom, renderPom(project, coordinate, metadata), StandardCharsets.UTF_8);
    uploads.add(pom);

    signer.sign(List.copyOf(uploads));
    List<Path> files = new ArrayList<>(uploads);
    for (Path upload : uploads) files.add(signaturePath(upload));
    List<Path> checksummed = List.copyOf(files);
    for (Path file : checksummed) {
      Files.writeString(checksumPath(file), sha256(file) + "\n", StandardCharsets.UTF_8);
      files.add(checksumPath(file));
    }

    if (!dryRun) {
      if (repository == null || repository.isBlank()) {
        throw new IllegalArgumentException(
            "missing publish repository; pass --repository or set JOT_PUBLISH_REPOSITORY");
      }
      publishFiles(repository, username, password, coordinate, mainJar, files);
    }
    return new Result(coordinate, stage, List.copyOf(files));
  }

  public static ArtifactSigner gpgSigner(String signingKey) {
    String selectedKey =
        signingKey == null || signingKey.isBlank()
            ? System.getenv("JOT_PUBLISH_GPG_KEY")
            : signingKey;
    return artifacts -> {
      for (Path artifact : artifacts) {
        Path signature = signaturePath(artifact);
        Files.deleteIfExists(signature);
        List<String> command =
            new ArrayList<>(
                List.of(
                    "gpg",
                    "--batch",
                    "--yes",
                    "--armor",
                    "--detach-sign",
                    "--output",
                    signature.toString()));
        if (selectedKey != null && !selectedKey.isBlank()) {
          command.addAll(List.of("--local-user", selectedKey));
        }
        command.add(artifact.toString());
        Process process = new ProcessBuilder(command).redirectErrorStream(true).start();
        String output = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
        if (process.waitFor() != 0)
          throw new IOException("gpg signing failed for " + artifact + ": " + output.strip());
      }
    };
  }

  private static MavenCoordinate coordinate(ProjectConfig config, boolean allowSnapshot) {
    String group = config.group();
    if (group == null || group.isBlank())
      throw new IllegalArgumentException("missing [project].group or inherited workspace group");
    validatePathSegment(config.name(), "artifact name");
    validatePathSegment(config.version(), "project version");
    for (String segment : group.split("\\.", -1)) validatePathSegment(segment, "group segment");
    if (!allowSnapshot && config.version().toUpperCase(Locale.ROOT).contains("SNAPSHOT"))
      throw new IllegalArgumentException(
          "refusing to publish snapshot version `"
              + config.version()
              + "` without --allow-snapshot");
    return new MavenCoordinate(group, config.name(), config.version(), "");
  }

  private static void validatePathSegment(String value, String description) {
    if (value == null
        || value.isBlank()
        || value.equals(".")
        || value.equals("..")
        || !value.matches("[A-Za-z0-9_][A-Za-z0-9_.-]*"))
      throw new IllegalArgumentException("invalid Maven " + description + ": " + value);
  }

  private static void validateMetadata(PublishMetadata metadata) {
    required(metadata.license(), "publish.license");
    required(metadata.description(), "publish.description");
    required(metadata.url(), "publish.url");
    required(metadata.scm(), "publish.scm");
    if (metadata.developer() == null)
      throw new IllegalArgumentException("publish.developer is required");
    required(metadata.developer().name(), "publish.developer.name");
  }

  private static void required(String value, String key) {
    if (value == null || value.isBlank()) throw new IllegalArgumentException(key + " is required");
  }

  private static String fileName(MavenCoordinate coordinate, String classifier, String extension) {
    return coordinate.artifact() + "-" + coordinate.version() + classifier + "." + extension;
  }

  private static void createSourcesJar(WorkspaceProject project, Path output) throws IOException {
    Path staging = Files.createTempDirectory(project.root().resolve("target"), ".jot-sources-");
    try {
      for (String sourceDirectory : project.config().sourceDirectories()) {
        Path sourceRoot = project.root().resolve(sourceDirectory).normalize();
        if (!sourceRoot.startsWith(project.root()))
          throw new IllegalArgumentException(
              "source directory escapes project: " + sourceDirectory);
        if (Files.isDirectory(sourceRoot)) copyContents(sourceRoot, staging);
      }
      createJar(staging, output);
    } finally {
      deleteTree(staging);
    }
  }

  private static void copyContents(Path source, Path destination) throws IOException {
    try (Stream<Path> paths = Files.walk(source)) {
      for (Path item : paths.sorted().toList()) {
        if (item.equals(source)) continue;
        Path target = destination.resolve(source.relativize(item));
        if (Files.isDirectory(item)) Files.createDirectories(target);
        else {
          Files.createDirectories(target.getParent());
          Files.copy(item, target);
        }
      }
    }
  }

  private static void createJar(Path source, Path output) throws IOException {
    Files.createDirectories(output.getParent());
    try (JarOutputStream jar = new JarOutputStream(Files.newOutputStream(output));
        Stream<Path> paths = Files.walk(source)) {
      for (Path file : paths.filter(Files::isRegularFile).sorted().toList()) {
        String name = source.relativize(file).toString().replace(java.io.File.separatorChar, '/');
        JarEntry entry = new JarEntry(name);
        entry.setTime(REPRODUCIBLE_JAR_TIME);
        jar.putNextEntry(entry);
        Files.copy(file, jar);
        jar.closeEntry();
      }
    }
  }

  private static String renderPom(
      WorkspaceProject project, MavenCoordinate coordinate, PublishMetadata metadata) {
    StringBuilder xml =
        new StringBuilder(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
                + "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"\n"
                + "         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n"
                + "         xsi:schemaLocation=\"http://maven.apache.org/POM/4.0.0"
                + " https://maven.apache.org/xsd/maven-4.0.0.xsd\">\n");
    element(xml, 1, "modelVersion", "4.0.0");
    element(xml, 1, "groupId", coordinate.group());
    element(xml, 1, "artifactId", coordinate.artifact());
    element(xml, 1, "version", coordinate.version());
    element(xml, 1, "packaging", "jar");
    element(xml, 1, "name", project.name());
    element(xml, 1, "description", metadata.description());
    element(xml, 1, "url", metadata.url());
    xml.append("  <licenses>\n    <license>\n");
    element(xml, 3, "name", metadata.license());
    xml.append("    </license>\n  </licenses>\n");
    xml.append("  <scm>\n");
    element(xml, 2, "url", metadata.scm());
    element(xml, 2, "connection", "scm:git:" + metadata.scm());
    xml.append("  </scm>\n  <developers>\n    <developer>\n");
    element(xml, 3, "name", metadata.developer().name());
    if (metadata.developer().email() != null && !metadata.developer().email().isBlank())
      element(xml, 3, "email", metadata.developer().email());
    xml.append("    </developer>\n  </developers>\n");

    List<Dependency> dependencies = new ArrayList<>();
    for (String notation : project.config().dependencies().values()) {
      MavenCoordinate item = MavenCoordinate.parse(notation);
      dependencies.add(
          new Dependency(item.group(), item.artifact(), item.version(), item.classifier()));
    }
    for (WorkspaceProject dependency : project.dependencies()) {
      String group = dependency.config().group();
      if (group == null || group.isBlank())
        throw new IllegalArgumentException(
            "path dependency " + dependency.name() + " is missing project.group for publish");
      dependencies.add(new Dependency(group, dependency.name(), dependency.config().version(), ""));
    }
    dependencies.sort(Comparator.comparing(Dependency::group).thenComparing(Dependency::artifact));
    if (!dependencies.isEmpty()) {
      xml.append("  <dependencies>\n");
      for (Dependency dependency : dependencies) {
        xml.append("    <dependency>\n");
        element(xml, 3, "groupId", dependency.group());
        element(xml, 3, "artifactId", dependency.artifact());
        element(xml, 3, "version", dependency.version());
        if (!dependency.classifier().isBlank())
          element(xml, 3, "classifier", dependency.classifier());
        xml.append("    </dependency>\n");
      }
      xml.append("  </dependencies>\n");
    }
    xml.append("</project>\n");
    return xml.toString();
  }

  private static void element(StringBuilder xml, int indent, String name, String value) {
    xml.append("  ".repeat(indent))
        .append('<')
        .append(name)
        .append('>')
        .append(escapeXml(value))
        .append("</")
        .append(name)
        .append(">\n");
  }

  private static String escapeXml(String value) {
    return value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;");
  }

  private static void publishFiles(
      String repository,
      String username,
      String password,
      MavenCoordinate coordinate,
      Path mainJar,
      List<Path> files)
      throws Exception {
    String relative =
        coordinate.group().replace('.', '/')
            + "/"
            + coordinate.artifact()
            + "/"
            + coordinate.version();
    boolean httpRepository = repository.startsWith("http://") || repository.startsWith("https://");
    URI base =
        httpRepository
            ? URI.create(repository.endsWith("/") ? repository : repository + "/")
            : null;
    if (!httpRepository) {
      Path root =
          repository.startsWith("file:") ? Path.of(URI.create(repository)) : Path.of(repository);
      root = root.toAbsolutePath().normalize();
      Path directory = root.resolve(relative).normalize();
      if (!directory.startsWith(root.toAbsolutePath().normalize()))
        throw new IllegalArgumentException("publish repository path escapes its root");
      if (Files.exists(directory.resolve(mainJar.getFileName())))
        throw new IOException(
            "artifact already exists in repository: " + directory.resolve(mainJar.getFileName()));
      Files.createDirectories(directory);
      for (Path file : files) Files.copy(file, directory.resolve(file.getFileName()));
      return;
    }
    HttpClient client =
        HttpClient.newBuilder().connectTimeout(java.time.Duration.ofSeconds(20)).build();
    URI main = base.resolve(relative + "/" + mainJar.getFileName());
    HttpRequest.Builder probeRequest =
        HttpRequest.newBuilder(main).method("HEAD", HttpRequest.BodyPublishers.noBody());
    addBasicAuthorization(probeRequest, username, password);
    HttpResponse<Void> probe =
        client.send(probeRequest.build(), HttpResponse.BodyHandlers.discarding());
    if (probe.statusCode() == 200)
      throw new IOException("artifact already exists in repository: " + main);
    if (probe.statusCode() != 404 && probe.statusCode() != 405)
      throw new IOException(
          "failed to probe repository (HTTP " + probe.statusCode() + ") at " + main);
    for (Path file : files) {
      URI uri = base.resolve(relative + "/" + file.getFileName());
      HttpRequest.Builder upload =
          HttpRequest.newBuilder(uri)
              .timeout(java.time.Duration.ofSeconds(120))
              .PUT(HttpRequest.BodyPublishers.ofFile(file));
      addBasicAuthorization(upload, username, password);
      HttpResponse<String> response =
          client.send(upload.build(), HttpResponse.BodyHandlers.ofString());
      if (response.statusCode() / 100 != 2)
        throw new IOException("upload failed for " + uri + " (HTTP " + response.statusCode() + ")");
    }
  }

  private static void addBasicAuthorization(
      HttpRequest.Builder request, String username, String password) {
    if (username == null) return;
    String credentials = username + ":" + (password == null ? "" : password);
    String encoded =
        java.util.Base64.getEncoder().encodeToString(credentials.getBytes(StandardCharsets.UTF_8));
    request.header("Authorization", "Basic " + encoded);
  }

  private static Path signaturePath(Path file) {
    return file.resolveSibling(file.getFileName() + ".asc");
  }

  private static Path checksumPath(Path file) {
    return file.resolveSibling(file.getFileName() + ".sha256");
  }

  private static String sha256(Path file) throws Exception {
    return HexFormat.of()
        .formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(file)));
  }

  private static void deleteTree(Path root) throws IOException {
    if (!Files.exists(root)) return;
    try (Stream<Path> paths = Files.walk(root)) {
      for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
    }
  }

  @FunctionalInterface
  public interface ArtifactSigner {
    void sign(List<Path> artifacts) throws Exception;
  }

  public record Result(MavenCoordinate coordinate, Path stagingDirectory, List<Path> files) {}

  private record Dependency(String group, String artifact, String version, String classifier) {}
}
