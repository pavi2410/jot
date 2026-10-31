package dev.jot.core.resolve;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;
import javax.xml.XMLConstants;
import javax.xml.parsers.DocumentBuilderFactory;
import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.Node;
import org.w3c.dom.NodeList;

/** Resolves pinned Maven coordinates, their compile/runtime POM dependencies, and JARs. */
public final class MavenResolver {
  private static final URI CENTRAL = URI.create("https://repo.maven.apache.org/maven2/");
  private static final Duration VERSION_METADATA_TTL = Duration.ofHours(6);
  private static final Duration SNAPSHOT_METADATA_TTL = Duration.ofMinutes(5);
  private final Path cacheRoot;
  private final URI repository;
  private final boolean offline;
  private final HttpClient client;
  private final Map<String, PomModel> pomModels = new LinkedHashMap<>();

  public MavenResolver() {
    this(
        Path.of(System.getProperty("user.home"), ".jot", "java-maven-cache"),
        repositoryFromEnvironment(),
        Boolean.getBoolean("jot.offline"));
  }

  public MavenResolver(Path cacheRoot, URI repository) {
    this(cacheRoot, repository, false);
  }

  public MavenResolver(Path cacheRoot, URI repository, boolean offline) {
    this.cacheRoot = cacheRoot.toAbsolutePath().normalize();
    String base = repository.toString().endsWith("/") ? repository.toString() : repository + "/";
    this.repository = URI.create(base);
    this.offline = offline;
    this.client = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(20)).build();
  }

  private static URI repositoryFromEnvironment() {
    String configured = System.getenv("JOT_MAVEN_REPOSITORY");
    return configured == null || configured.isBlank() ? CENTRAL : URI.create(configured);
  }

  public static URI defaultRepository() {
    return repositoryFromEnvironment();
  }

  public List<ResolvedArtifact> resolve(Map<String, String> declaredDependencies) throws Exception {
    return resolve(declaredDependencies, Integer.MAX_VALUE);
  }

  public List<ResolvedArtifact> resolve(Map<String, String> declaredDependencies, int maxDepth)
      throws Exception {
    if (maxDepth < 0) throw new IllegalArgumentException("dependency depth cannot be negative");
    ArrayDeque<DependencyRequest> pending = new ArrayDeque<>();
    for (String notation : declaredDependencies.values()) {
      pending.addLast(new DependencyRequest(MavenCoordinate.parse(notation), Set.of(), 0));
    }
    Map<String, ResolvedArtifact> selected = new LinkedHashMap<>();
    while (!pending.isEmpty()) {
      DependencyRequest request = pending.removeFirst();
      MavenCoordinate coordinate = resolveVersion(request.coordinate);
      if (request.exclusions.stream().anyMatch(exclusion -> exclusion.matches(coordinate)))
        continue;
      String moduleKey = coordinate.moduleKey();
      if (selected.containsKey(moduleKey)) continue; // nearest dependency wins

      Path jar = resolveJar(coordinate);
      selected.put(moduleKey, new ResolvedArtifact(coordinate, jar));
      PomModel model = model(coordinate, new LinkedHashSet<>());
      if (request.depth >= maxDepth) continue;
      for (PomDependency dependency : model.dependencies) {
        if (!dependency.isRuntimeVisible() || dependency.optional) continue;
        if (request.exclusions.stream().anyMatch(exclusion -> exclusion.matches(dependency)))
          continue;
        Set<Exclusion> exclusions = new LinkedHashSet<>(request.exclusions);
        exclusions.addAll(dependency.exclusions);
        pending.addLast(
            new DependencyRequest(
                dependency.coordinate, Set.copyOf(exclusions), request.depth + 1));
      }
    }
    return List.copyOf(selected.values());
  }

  public Optional<String> latestAvailableVersion(MavenCoordinate coordinate) throws Exception {
    ArtifactMetadata metadata = metadata(coordinate);
    return Optional.ofNullable(
        MavenVersions.best(metadata.release(), metadata.latest(), metadata.versions()));
  }

  private MavenCoordinate resolveVersion(MavenCoordinate coordinate) throws Exception {
    String version = coordinate.version();
    if (version.contains("${"))
      throw new IOException("unsupported Maven version expression: " + coordinate.notation());
    if (!MavenVersions.isDynamic(version)) return coordinate;
    ArtifactMetadata metadata = metadata(coordinate);
    String resolved =
        MavenVersions.resolve(version, metadata.release(), metadata.latest(), metadata.versions());
    if (resolved == null)
      throw new IOException(
          "no version in Maven metadata for "
              + coordinate.group()
              + ":"
              + coordinate.artifact()
              + " matches "
              + version);
    return new MavenCoordinate(
        coordinate.group(), coordinate.artifact(), resolved, coordinate.classifier());
  }

  private ArtifactMetadata metadata(MavenCoordinate coordinate) throws Exception {
    String metadataPath =
        coordinate.group().replace('.', '/') + "/" + coordinate.artifact() + "/maven-metadata.xml";
    byte[] bytes = fetch(repository.resolve(metadataPath), true, VERSION_METADATA_TTL);
    Element versioning = child(parseXml(bytes).getDocumentElement(), "versioning");
    List<String> versions =
        children(child(versioning, "versions"), "version").stream()
            .map(Element::getTextContent)
            .map(String::trim)
            .filter(value -> !value.isEmpty())
            .toList();
    return new ArtifactMetadata(text(versioning, "latest"), text(versioning, "release"), versions);
  }

  public List<TreeEntry> resolveDependencyTree(MavenCoordinate root, int maxDepth)
      throws Exception {
    if (maxDepth < 0) throw new IllegalArgumentException("tree depth cannot be negative");
    List<TreeEntry> entries = new ArrayList<>();
    root = resolveVersion(root);
    entries.add(new TreeEntry(0, root, "", false, null));
    Set<String> ancestors = new LinkedHashSet<>();
    ancestors.add(root.notation());
    appendTree(root, 1, maxDepth, Set.of(), ancestors, entries);
    return List.copyOf(entries);
  }

  /** Resolves a pinned coordinate by loading its effective POM model. */
  public MavenCoordinate resolveCoordinate(String notation) throws Exception {
    MavenCoordinate coordinate = resolveVersion(MavenCoordinate.parse(notation));
    model(coordinate, new LinkedHashSet<>());
    return coordinate;
  }

  /** Returns the effective direct dependencies declared by a pinned artifact's POM. */
  public List<DirectDependency> resolveDirectDependencies(MavenCoordinate coordinate)
      throws Exception {
    return model(coordinate, new LinkedHashSet<>()).dependencies().stream()
        .map(
            dependency ->
                new DirectDependency(
                    dependency.coordinate(), dependency.scope(), dependency.optional()))
        .toList();
  }

  public record DirectDependency(MavenCoordinate coordinate, String scope, boolean optional) {}

  private void appendTree(
      MavenCoordinate parent,
      int depth,
      int maxDepth,
      Set<Exclusion> inheritedExclusions,
      Set<String> ancestors,
      List<TreeEntry> entries)
      throws Exception {
    if (depth > maxDepth) return;
    for (PomDependency dependency : model(parent, new LinkedHashSet<>()).dependencies) {
      MavenCoordinate coordinate = resolveVersion(dependency.coordinate);
      String note = null;
      if (!dependency.isRuntimeVisible()) note = "scope omitted";
      else if (inheritedExclusions.stream().anyMatch(exclusion -> exclusion.matches(coordinate))) {
        note = "excluded";
      } else if (dependency.optional && depth > 1) {
        note = "optional omitted";
      } else if (ancestors.contains(coordinate.notation())) {
        note = "cycle detected";
      }

      entries.add(new TreeEntry(depth, coordinate, dependency.scope, dependency.optional, note));
      if (note == null) {
        Set<Exclusion> exclusions = new LinkedHashSet<>(inheritedExclusions);
        exclusions.addAll(dependency.exclusions);
        ancestors.add(coordinate.notation());
        try {
          appendTree(coordinate, depth + 1, maxDepth, Set.copyOf(exclusions), ancestors, entries);
        } finally {
          ancestors.remove(coordinate.notation());
        }
      }
    }
  }

  public record TreeEntry(
      int depth, MavenCoordinate coordinate, String scope, boolean optional, String note) {}

  private Path resolveJar(MavenCoordinate coordinate) throws Exception {
    URI uri = resolveSnapshotUri(coordinate, "jar");
    byte[] jar = fetch(uri, true);
    byte[] checksum = fetch(URI.create(uri + ".sha256"), false);
    if (checksum != null) {
      String expected = new String(checksum, StandardCharsets.US_ASCII).trim().split("\\s+")[0];
      String actual = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(jar));
      if (!expected.equalsIgnoreCase(actual)) {
        throw new IOException("SHA-256 mismatch for " + coordinate.notation());
      }
    }
    Path cached = cachePath(uri);
    if (!Files.isRegularFile(cached)) store(cached, jar);
    return cached;
  }

  private PomModel model(MavenCoordinate coordinate, Set<String> visiting) throws Exception {
    String key = coordinate.notation();
    PomModel cachedModel = pomModels.get(key);
    if (cachedModel != null) return cachedModel;
    if (!visiting.add(key)) throw new IOException("cyclic Maven POM parent/BOM: " + key);

    URI pomUri = resolveSnapshotUri(coordinate, "pom");
    byte[] pomBytes = fetch(pomUri, false);
    if (pomBytes == null) {
      if (offline) throw new IOException("offline mode: POM is not cached: " + pomUri);
      PomModel empty = PomModel.empty();
      pomModels.put(key, empty);
      visiting.remove(key);
      return empty;
    }
    Element project = parseXml(pomBytes).getDocumentElement();

    Element parentElement = child(project, "parent");
    PomModel parent = PomModel.empty();
    if (parentElement != null) {
      String parentGroup = text(parentElement, "groupId");
      String parentArtifact = text(parentElement, "artifactId");
      String parentVersion = text(parentElement, "version");
      if (parentGroup != null && parentArtifact != null && parentVersion != null) {
        parent =
            model(
                resolveVersion(new MavenCoordinate(parentGroup, parentArtifact, parentVersion, "")),
                visiting);
      }
    }

    Map<String, String> properties = new LinkedHashMap<>(parent.properties);
    Element propertiesElement = child(project, "properties");
    for (Element property : children(propertiesElement, null)) {
      properties.put(localName(property), property.getTextContent().trim());
    }
    String group = text(project, "groupId");
    String artifact = text(project, "artifactId");
    String version = text(project, "version");
    group = interpolate(group == null ? coordinate.group() : group, properties);
    artifact = interpolate(artifact == null ? coordinate.artifact() : artifact, properties);
    version = interpolate(version == null ? coordinate.version() : version, properties);
    addProjectProperties(properties, group, artifact, version);

    Map<String, String> managedVersions = new LinkedHashMap<>(parent.managedVersions);
    Element management = child(child(project, "dependencyManagement"), "dependencies");
    for (Element dependencyElement : children(management, "dependency")) {
      PomDependency dependency = parseDependency(dependencyElement, properties, managedVersions);
      if (dependency == null) continue;
      if (dependency.scope.equals("import") && dependency.type.equals("pom")) {
        PomModel imported = model(resolveVersion(dependency.coordinate), visiting);
        managedVersions.putAll(imported.managedVersions);
      } else {
        managedVersions.put(managementKey(dependency.coordinate), dependency.coordinate.version());
      }
    }

    List<PomDependency> dependencies = new ArrayList<>(parent.dependencies);
    for (Element dependencyElement : children(child(project, "dependencies"), "dependency")) {
      PomDependency dependency = parseDependency(dependencyElement, properties, managedVersions);
      if (dependency != null) dependencies.add(dependency);
    }
    PomModel result =
        new PomModel(
            Map.copyOf(properties), Map.copyOf(managedVersions), List.copyOf(dependencies));
    pomModels.put(key, result);
    visiting.remove(key);
    return result;
  }

  private PomDependency parseDependency(
      Element element, Map<String, String> properties, Map<String, String> managedVersions)
      throws IOException {
    String group = interpolate(text(element, "groupId"), properties);
    String artifact = interpolate(text(element, "artifactId"), properties);
    if (group == null || artifact == null) return null;
    String type = Objects.requireNonNullElse(interpolate(text(element, "type"), properties), "jar");
    String classifier =
        Objects.requireNonNullElse(interpolate(text(element, "classifier"), properties), "");
    String scope =
        Objects.requireNonNullElse(interpolate(text(element, "scope"), properties), "compile");
    String version = interpolate(text(element, "version"), properties);
    if (version == null) version = managedVersions.get(group + ":" + artifact + ":" + classifier);
    if (version == null || version.contains("${")) {
      if (scope.equals("test") || scope.equals("provided") || scope.equals("system")) return null;
      throw new IOException("no resolved version for Maven dependency " + group + ":" + artifact);
    }
    boolean optional = "true".equalsIgnoreCase(text(element, "optional"));
    if (!type.equals("jar")) return null;
    Set<Exclusion> exclusions = new LinkedHashSet<>();
    for (Element exclusion : children(child(element, "exclusions"), "exclusion")) {
      String excludedGroup = text(exclusion, "groupId");
      String excludedArtifact = text(exclusion, "artifactId");
      if (excludedGroup != null && excludedArtifact != null) {
        exclusions.add(
            new Exclusion(
                interpolate(excludedGroup, properties), interpolate(excludedArtifact, properties)));
      }
    }
    return new PomDependency(
        new MavenCoordinate(group, artifact, version, classifier),
        scope,
        type,
        optional,
        Set.copyOf(exclusions));
  }

  private URI resolveSnapshotUri(MavenCoordinate coordinate, String extension) throws Exception {
    String version = coordinate.version();
    if (!version.toUpperCase(java.util.Locale.ROOT).endsWith("-SNAPSHOT"))
      return repository.resolve(
          extension.equals("pom") ? coordinate.pomPath() : coordinate.repositoryPath(extension));

    String versionDirectory =
        coordinate.group().replace('.', '/') + "/" + coordinate.artifact() + "/" + version + "/";
    URI metadataUri = repository.resolve(versionDirectory + "maven-metadata.xml");
    byte[] bytes = fetch(metadataUri, false, SNAPSHOT_METADATA_TTL);
    String resolvedVersion = null;
    String requestedClassifier = extension.equals("pom") ? "" : coordinate.classifier();
    String timestamp = null;
    String buildNumber = null;
    if (bytes != null) {
      Element versioning = child(parseXml(bytes).getDocumentElement(), "versioning");
      Element snapshot = child(versioning, "snapshot");
      timestamp = text(snapshot, "timestamp");
      buildNumber = text(snapshot, "buildNumber");
      for (Element item : children(child(versioning, "snapshotVersions"), "snapshotVersion")) {
        String itemExtension = text(item, "extension");
        String itemClassifier = Objects.requireNonNullElse(text(item, "classifier"), "");
        if (extension.equals(itemExtension) && requestedClassifier.equals(itemClassifier)) {
          resolvedVersion = text(item, "value");
          if (resolvedVersion != null) break;
        }
      }
    }
    if (resolvedVersion == null && timestamp != null && buildNumber != null) {
      String base = version.substring(0, version.length() - "-SNAPSHOT".length());
      resolvedVersion = base + "-" + timestamp + "-" + buildNumber;
    }
    if (resolvedVersion == null)
      return repository.resolve(
          versionDirectory
              + coordinate.artifact()
              + "-"
              + version
              + classifierAndExtension(requestedClassifier, extension));
    return repository.resolve(
        versionDirectory
            + coordinate.artifact()
            + "-"
            + resolvedVersion
            + classifierAndExtension(requestedClassifier, extension));
  }

  private static String classifierAndExtension(String classifier, String extension) {
    return (classifier.isEmpty() ? "" : "-" + classifier) + "." + extension;
  }

  private byte[] fetch(URI uri, boolean required) throws IOException, InterruptedException {
    return fetch(uri, required, null);
  }

  private byte[] fetch(URI uri, boolean required, Duration cacheTtl)
      throws IOException, InterruptedException {
    Path cached = cachePath(uri);
    if (Files.isRegularFile(cached)) {
      boolean fresh =
          cacheTtl == null
              || Files.getLastModifiedTime(cached)
                  .toInstant()
                  .plus(cacheTtl)
                  .isAfter(Instant.now());
      if (offline || fresh) return Files.readAllBytes(cached);
    }
    if (offline) {
      if (required) throw new IOException("offline mode: artifact is not cached: " + uri);
      return null;
    }
    byte[] data;
    if (uri.getScheme().equals("file")) {
      Path file = Path.of(uri);
      if (!Files.isRegularFile(file)) {
        if (required) throw new IOException("Maven artifact not found: " + uri);
        return null;
      }
      data = Files.readAllBytes(file);
    } else if (uri.getScheme().equals("https") || uri.getScheme().equals("http")) {
      HttpRequest request =
          HttpRequest.newBuilder(uri).timeout(Duration.ofSeconds(60)).GET().build();
      HttpResponse<byte[]> response = client.send(request, HttpResponse.BodyHandlers.ofByteArray());
      if (response.statusCode() == 404) {
        if (required) throw new IOException("Maven artifact not found: " + uri);
        return null;
      }
      if (response.statusCode() < 200 || response.statusCode() >= 300) {
        throw new IOException(
            "Maven repository returned HTTP " + response.statusCode() + " for " + uri);
      }
      data = response.body();
    } else {
      throw new IOException("unsupported Maven repository scheme: " + uri.getScheme());
    }
    store(cached, data);
    return data;
  }

  private Path cachePath(URI uri) {
    String relative = uri.getPath();
    if (relative == null || relative.isBlank()) {
      relative = HexFormat.of().formatHex(uri.toString().getBytes(StandardCharsets.UTF_8));
    }
    while (relative.startsWith("/")) relative = relative.substring(1);
    Path result = cacheRoot.resolve(relative).normalize();
    if (!result.startsWith(cacheRoot))
      throw new IllegalArgumentException("invalid repository path");
    return result;
  }

  private static void store(Path file, byte[] data) throws IOException {
    Files.createDirectories(file.getParent());
    Path temporary = Files.createTempFile(file.getParent(), ".jot-download-", ".tmp");
    try {
      Files.write(temporary, data);
      try {
        Files.move(
            temporary, file, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
      } catch (AtomicMoveNotSupportedException ignored) {
        Files.move(temporary, file, StandardCopyOption.REPLACE_EXISTING);
      }
    } finally {
      Files.deleteIfExists(temporary);
    }
  }

  private static Document parseXml(byte[] bytes) throws Exception {
    DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
    factory.setNamespaceAware(true);
    factory.setXIncludeAware(false);
    factory.setExpandEntityReferences(false);
    factory.setFeature(XMLConstants.FEATURE_SECURE_PROCESSING, true);
    factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
    factory.setAttribute(XMLConstants.ACCESS_EXTERNAL_DTD, "");
    factory.setAttribute(XMLConstants.ACCESS_EXTERNAL_SCHEMA, "");
    return factory.newDocumentBuilder().parse(new java.io.ByteArrayInputStream(bytes));
  }

  private static void addProjectProperties(
      Map<String, String> properties, String group, String artifact, String version) {
    properties.put("project.groupId", group);
    properties.put("pom.groupId", group);
    properties.put("project.artifactId", artifact);
    properties.put("pom.artifactId", artifact);
    properties.put("project.version", version);
    properties.put("pom.version", version);
  }

  private static String interpolate(String value, Map<String, String> properties) {
    if (value == null) return null;
    String result = value;
    for (int iteration = 0; iteration < 20; iteration++) {
      int opening = result.indexOf("${");
      if (opening < 0) return result;
      int closing = result.indexOf('}', opening + 2);
      if (closing < 0) return result;
      String key = result.substring(opening + 2, closing);
      String replacement = properties.get(key);
      if (replacement == null) return result;
      result = result.substring(0, opening) + replacement + result.substring(closing + 1);
    }
    return result;
  }

  private static String managementKey(MavenCoordinate coordinate) {
    return coordinate.group() + ":" + coordinate.artifact() + ":" + coordinate.classifier();
  }

  private static Element child(Element parent, String name) {
    if (parent == null) return null;
    for (Element element : children(parent, null)) {
      if (localName(element).equals(name)) return element;
    }
    return null;
  }

  private static List<Element> children(Element parent, String name) {
    if (parent == null) return List.of();
    List<Element> result = new ArrayList<>();
    NodeList nodes = parent.getChildNodes();
    for (int i = 0; i < nodes.getLength(); i++) {
      Node node = nodes.item(i);
      if (node instanceof Element element && (name == null || localName(element).equals(name))) {
        result.add(element);
      }
    }
    return result;
  }

  private static String text(Element parent, String name) {
    Element element = child(parent, name);
    if (element == null) return null;
    String value = element.getTextContent().trim();
    return value.isEmpty() ? null : value;
  }

  private static String localName(Element element) {
    return element.getLocalName() == null ? element.getNodeName() : element.getLocalName();
  }

  private record DependencyRequest(
      MavenCoordinate coordinate, Set<Exclusion> exclusions, int depth) {}

  private record Exclusion(String group, String artifact) {
    boolean matches(MavenCoordinate coordinate) {
      return (group.equals("*") || group.equals(coordinate.group()))
          && (artifact.equals("*") || artifact.equals(coordinate.artifact()));
    }

    boolean matches(PomDependency dependency) {
      return matches(dependency.coordinate);
    }
  }

  private record PomDependency(
      MavenCoordinate coordinate,
      String scope,
      String type,
      boolean optional,
      Set<Exclusion> exclusions) {
    boolean isRuntimeVisible() {
      return scope.equals("compile") || scope.equals("runtime");
    }
  }

  private record PomModel(
      Map<String, String> properties,
      Map<String, String> managedVersions,
      List<PomDependency> dependencies) {
    static PomModel empty() {
      return new PomModel(Map.of(), Map.of(), List.of());
    }
  }

  private record ArtifactMetadata(String latest, String release, List<String> versions) {}

  public record ResolvedArtifact(MavenCoordinate coordinate, Path jar) {}
}
