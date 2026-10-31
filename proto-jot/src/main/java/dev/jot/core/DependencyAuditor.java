package dev.jot.core;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import dev.jot.core.config.ManifestEditor;
import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
import dev.jot.core.resolve.MavenResolver.TreeEntry;
import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

/** Checks a resolved Maven graph against the OSV vulnerability database. */
public final class DependencyAuditor {
  private static final ObjectMapper JSON = new ObjectMapper();
  private final HttpClient client;
  private final URI osvBase;

  public DependencyAuditor() {
    this(
        HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(20)).build(),
        URI.create("https://api.osv.dev"));
  }

  DependencyAuditor(HttpClient client, URI osvBase) {
    this.client = client;
    this.osvBase = URI.create(osvBase.toString().replaceAll("/+$", ""));
  }

  public Report audit(
      Path projectRoot,
      ProjectConfig config,
      ProjectWorkspace workspace,
      MavenResolver resolver,
      boolean fix)
      throws Exception {
    Map<String, PackageContext> packages = collectPackages(workspace, resolver);
    List<PackageContext> ordered =
        packages.values().stream()
            .sorted(Comparator.comparing(item -> item.coordinate().notation()))
            .toList();
    if (ordered.isEmpty()) return new Report(0, List.of(), 0);

    JsonNode response = post("/v1/querybatch", batchRequest(ordered));
    JsonNode results = response.path("results");
    if (!results.isArray() || results.size() != ordered.size()) {
      throw new IOException(
          "OSV returned " + results.size() + " results for " + ordered.size() + " packages");
    }
    List<List<String>> vulnerabilityIds = new ArrayList<>();
    Set<String> uniqueIds = new TreeSet<>();
    for (JsonNode result : results) {
      List<String> ids = new ArrayList<>();
      for (JsonNode vulnerability : result.path("vulns")) {
        String id = vulnerability.path("id").asText(null);
        if (id != null && !id.isBlank()) {
          ids.add(id);
          uniqueIds.add(id);
        }
      }
      vulnerabilityIds.add(List.copyOf(ids));
    }

    Map<String, JsonNode> details = new LinkedHashMap<>();
    for (String id : uniqueIds) {
      String path =
          "/v1/vulns/" + URLEncoder.encode(id, StandardCharsets.UTF_8).replace("+", "%20");
      details.put(id, get(path));
    }

    List<Finding> findings = new ArrayList<>();
    for (int index = 0; index < ordered.size(); index++) {
      PackageContext item = ordered.get(index);
      for (String id : vulnerabilityIds.get(index)) {
        JsonNode vulnerability = details.get(id);
        findings.add(
            new Finding(
                severity(vulnerability),
                vulnerability.path("id").asText(id),
                item.coordinate(),
                fixedVersion(vulnerability, item.coordinate()),
                vulnerability.path("summary").asText("No summary provided"),
                List.copyOf(item.members()),
                List.copyOf(item.chains())));
      }
    }
    findings.sort(
        Comparator.comparingInt((Finding finding) -> finding.severity().rank())
            .reversed()
            .thenComparing(finding -> finding.packageCoordinate().notation())
            .thenComparing(Finding::id));
    int fixed = fix ? applyFixes(projectRoot, workspace, findings, resolver) : 0;
    return new Report(ordered.size(), List.copyOf(findings), fixed);
  }

  private static ObjectNode batchRequest(List<PackageContext> packages) {
    ObjectNode request = JSON.createObjectNode();
    ArrayNode queries = request.putArray("queries");
    for (PackageContext item : packages) {
      MavenCoordinate coordinate = item.coordinate();
      ObjectNode query = queries.addObject();
      query.put("version", coordinate.version());
      ObjectNode packageNode = query.putObject("package");
      packageNode.put("ecosystem", "Maven");
      packageNode.put("name", coordinate.group() + ":" + coordinate.artifact());
    }
    return request;
  }

  private JsonNode post(String path, JsonNode body) throws Exception {
    HttpRequest request =
        HttpRequest.newBuilder(osvBase.resolve(path))
            .timeout(Duration.ofSeconds(40))
            .header("Content-Type", "application/json")
            .POST(HttpRequest.BodyPublishers.ofString(JSON.writeValueAsString(body)))
            .build();
    return send(path, request);
  }

  private JsonNode get(String path) throws Exception {
    HttpRequest request =
        HttpRequest.newBuilder(osvBase.resolve(path)).timeout(Duration.ofSeconds(40)).GET().build();
    return send(path, request);
  }

  private JsonNode send(String path, HttpRequest request) throws Exception {
    HttpResponse<String> response =
        client.send(request, HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8));
    if (response.statusCode() / 100 != 2) {
      throw new IOException(
          "OSV request failed with HTTP " + response.statusCode() + " for " + path);
    }
    return JSON.readTree(response.body());
  }

  private static Map<String, PackageContext> collectPackages(
      ProjectWorkspace workspace, MavenResolver resolver) throws Exception {
    Map<String, PackageContext> packages = new LinkedHashMap<>();
    for (WorkspaceProject project : workspace.ordered()) {
      for (String root : project.config().dependencies().values()) {
        List<TreeEntry> tree = resolver.resolveDependencyTree(MavenCoordinate.parse(root), 8);
        List<MavenCoordinate> chain = new ArrayList<>();
        for (TreeEntry entry : tree) {
          while (chain.size() > entry.depth()) chain.removeLast();
          chain.add(entry.coordinate());
          String key = entry.coordinate().notation();
          PackageContext item =
              packages.computeIfAbsent(key, ignored -> new PackageContext(entry.coordinate()));
          item.members().add(project.name());
          List<MavenCoordinate> path = List.copyOf(chain);
          if (!item.chains().contains(path)) item.chains().add(path);
        }
      }
    }
    return packages;
  }

  private static AuditSeverity severity(JsonNode vulnerability) {
    AuditSeverity result = AuditSeverity.UNKNOWN;
    result = max(result, severityArray(vulnerability.path("severity")));
    result = max(result, holderSeverity(vulnerability.path("ecosystem_specific")));
    result = max(result, holderSeverity(vulnerability.path("database_specific")));
    for (JsonNode affected : vulnerability.path("affected")) {
      result = max(result, severityArray(affected.path("severity")));
      result = max(result, holderSeverity(affected.path("ecosystem_specific")));
      result = max(result, holderSeverity(affected.path("database_specific")));
    }
    return result;
  }

  private static AuditSeverity severityArray(JsonNode values) {
    AuditSeverity result = AuditSeverity.UNKNOWN;
    for (JsonNode value : values) result = max(result, fromScore(value.path("score").asText()));
    return result;
  }

  private static AuditSeverity holderSeverity(JsonNode value) {
    return fromLabel(value.path("severity").asText());
  }

  private static AuditSeverity fromScore(String value) {
    try {
      double score = Double.parseDouble(value.trim());
      if (score >= 9) return AuditSeverity.CRITICAL;
      if (score >= 7) return AuditSeverity.HIGH;
      if (score >= 4) return AuditSeverity.MODERATE;
      if (score > 0) return AuditSeverity.LOW;
    } catch (NumberFormatException ignored) {
      if (value.startsWith("CVSS:3.")) {
        Double score = cvssV3(value);
        if (score != null) return fromScore(Double.toString(score));
      }
      return fromLabel(value);
    }
    return AuditSeverity.UNKNOWN;
  }

  private static AuditSeverity fromLabel(String label) {
    return switch (label.trim().toUpperCase()) {
      case "LOW" -> AuditSeverity.LOW;
      case "MODERATE", "MEDIUM", "MEDIUM_LOW", "MEDIUM_HIGH" -> AuditSeverity.MODERATE;
      case "HIGH" -> AuditSeverity.HIGH;
      case "CRITICAL" -> AuditSeverity.CRITICAL;
      default -> AuditSeverity.UNKNOWN;
    };
  }

  private static Double cvssV3(String vector) {
    Map<String, String> metrics = new LinkedHashMap<>();
    for (String entry : vector.split("/")) {
      int colon = entry.indexOf(':');
      if (colon > 0) metrics.put(entry.substring(0, colon), entry.substring(colon + 1));
    }
    double av =
        switch (metrics.getOrDefault("AV", "")) {
          case "N" -> 0.85;
          case "A" -> 0.62;
          case "L" -> 0.55;
          case "P" -> 0.20;
          default -> -1;
        };
    double ac =
        switch (metrics.getOrDefault("AC", "")) {
          case "L" -> 0.77;
          case "H" -> 0.44;
          default -> -1;
        };
    String scope = metrics.get("S");
    String pr = metrics.get("PR");
    double privileges =
        switch (pr == null ? "" : pr + ":" + scope) {
          case "N:U", "N:C" -> 0.85;
          case "L:U" -> 0.62;
          case "L:C" -> 0.68;
          case "H:U" -> 0.27;
          case "H:C" -> 0.50;
          default -> -1;
        };
    double ui =
        switch (metrics.getOrDefault("UI", "")) {
          case "N" -> 0.85;
          case "R" -> 0.62;
          default -> -1;
        };
    double c = impact(metrics.get("C"));
    double i = impact(metrics.get("I"));
    double a = impact(metrics.get("A"));
    if (av < 0 || ac < 0 || privileges < 0 || ui < 0 || c < 0 || i < 0 || a < 0) return null;
    double impactSub = 1 - (1 - c) * (1 - i) * (1 - a);
    double impact =
        "U".equals(scope)
            ? 6.42 * impactSub
            : "C".equals(scope)
                ? 7.52 * (impactSub - 0.029) - 3.25 * Math.pow(impactSub - 0.02, 15)
                : -1;
    if (impact < 0) return null;
    if (impact == 0) return 0.0;
    double exploitability = 8.22 * av * ac * privileges * ui;
    double score =
        "U".equals(scope)
            ? Math.min(10, impact + exploitability)
            : Math.min(10, 1.08 * (impact + exploitability));
    return Math.ceil(score * 10 - 1e-10) / 10;
  }

  private static double impact(String value) {
    return switch (value == null ? "" : value) {
      case "H" -> 0.56;
      case "L" -> 0.22;
      case "N" -> 0;
      default -> -1;
    };
  }

  private static String fixedVersion(JsonNode vulnerability, MavenCoordinate coordinate) {
    String name = coordinate.group() + ":" + coordinate.artifact();
    List<String> fixedVersions = new ArrayList<>();
    for (JsonNode affected : vulnerability.path("affected")) {
      if (!affected.path("package").path("name").asText().equals(name)) continue;
      for (JsonNode range : affected.path("ranges")) {
        for (JsonNode event : range.path("events")) {
          String fixed = event.path("fixed").asText(null);
          if (fixed != null && !fixed.isBlank()) fixedVersions.add(fixed);
        }
      }
    }
    return fixedVersions.stream().sorted().findFirst().orElse(null);
  }

  private static int applyFixes(
      Path projectRoot, ProjectWorkspace workspace, List<Finding> findings, MavenResolver resolver)
      throws Exception {
    Map<String, String> replacements = new LinkedHashMap<>();
    for (Finding finding : findings) {
      if (finding.fixedVersion() != null) {
        replacements.putIfAbsent(
            finding.packageCoordinate().group() + ":" + finding.packageCoordinate().artifact(),
            finding.fixedVersion());
      }
    }
    int updated = 0;
    for (WorkspaceProject project : workspace.ordered()) {
      Path manifest = project.root().resolve("jot.toml");
      updated += updateSection(manifest, project.config().dependencies(), false, replacements);
      updated += updateSection(manifest, project.config().testDependencies(), true, replacements);
    }
    if (updated > 0) {
      ProjectConfig reloaded = ProjectConfig.read(projectRoot.resolve("jot.toml"), null);
      ProjectWorkspace refreshed =
          reloaded.isWorkspace()
              ? ProjectWorkspace.load(projectRoot, reloaded)
              : ProjectWorkspace.loadProject(projectRoot, reloaded);
      BuildEngine.lock(refreshed, projectRoot.resolve("jot.lock"), resolver);
    }
    return updated;
  }

  private static int updateSection(
      Path manifest,
      Map<String, String> declarations,
      boolean test,
      Map<String, String> replacements)
      throws IOException {
    int updated = 0;
    for (Map.Entry<String, String> declaration : declarations.entrySet()) {
      MavenCoordinate coordinate;
      try {
        coordinate = MavenCoordinate.parse(declaration.getValue());
      } catch (IllegalArgumentException ignored) {
        continue;
      }
      String version = replacements.get(coordinate.group() + ":" + coordinate.artifact());
      if (version == null || version.equals(coordinate.version())) continue;
      ManifestEditor.addDependency(
          manifest,
          declaration.getKey(),
          coordinate.group()
              + ":"
              + coordinate.artifact()
              + ":"
              + version
              + (coordinate.classifier().isBlank() ? "" : ":" + coordinate.classifier()),
          test);
      updated++;
    }
    return updated;
  }

  private static AuditSeverity max(AuditSeverity left, AuditSeverity right) {
    return left.rank() >= right.rank() ? left : right;
  }

  public enum AuditSeverity {
    UNKNOWN,
    LOW,
    MODERATE,
    HIGH,
    CRITICAL;

    int rank() {
      return ordinal();
    }

    public boolean failsCi() {
      return this == HIGH || this == CRITICAL;
    }
  }

  public record Finding(
      AuditSeverity severity,
      String id,
      MavenCoordinate packageCoordinate,
      String fixedVersion,
      String summary,
      List<String> members,
      List<List<MavenCoordinate>> chains) {}

  public record Report(int packagesScanned, List<Finding> findings, int fixedDependencies) {}

  private static final class PackageContext {
    private final MavenCoordinate coordinate;
    private final Set<String> members = new LinkedHashSet<>();
    private final List<List<MavenCoordinate>> chains = new ArrayList<>();

    private PackageContext(MavenCoordinate coordinate) {
      this.coordinate = coordinate;
    }

    MavenCoordinate coordinate() {
      return coordinate;
    }

    Set<String> members() {
      return members;
    }

    List<List<MavenCoordinate>> chains() {
      return chains;
    }
  }
}
