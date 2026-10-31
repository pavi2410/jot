package dev.jot.core.resolve;

import dev.jot.core.config.TomlDocument;
import dev.jot.core.config.TomlWriter;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Shared `jot.lock` representation for reproducible JVM dependency resolution. */
public record Lockfile(List<MavenCoordinate> roots, List<LockedPackage> packages) {
  public static Lockfile create(
      Iterable<String> declaredRoots, List<MavenResolver.ResolvedArtifact> artifacts)
      throws Exception {
    List<MavenCoordinate> roots = new ArrayList<>();
    for (String root : declaredRoots) roots.add(MavenCoordinate.parse(root));
    List<LockedPackage> packages = new ArrayList<>();
    for (MavenResolver.ResolvedArtifact artifact : artifacts) {
      byte[] bytes = Files.readAllBytes(artifact.jar());
      String checksum =
          HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
      packages.add(new LockedPackage(artifact.coordinate(), checksum));
    }
    roots.sort(Comparator.comparing(MavenCoordinate::notation));
    packages.sort(Comparator.comparing(item -> item.coordinate().notation()));
    return new Lockfile(List.copyOf(roots), List.copyOf(packages));
  }

  public static Lockfile read(Path file) throws IOException {
    if (!Files.isRegularFile(file)) throw new IOException("lockfile not found: " + file);
    TomlDocument document = TomlDocument.read(file);
    if (document.integer("version", -1) != 1) {
      throw new IOException("unsupported lockfile version in " + file);
    }
    List<MavenCoordinate> parsedRoots = new ArrayList<>();
    for (Map<String, Object> root : document.tableArray("roots")) {
      parsedRoots.add(coordinate(root, file));
    }
    List<LockedPackage> parsedPackages = new ArrayList<>();
    for (Map<String, Object> item : document.tableArray("package")) {
      String checksum = string(item, "sha256");
      if (checksum == null || !checksum.matches("[0-9a-fA-F]{64}")) {
        throw new IOException("invalid sha256 entry in " + file);
      }
      parsedPackages.add(new LockedPackage(coordinate(item, file), checksum.toLowerCase()));
    }
    return new Lockfile(List.copyOf(parsedRoots), List.copyOf(parsedPackages));
  }

  public void write(Path file) throws IOException {
    TomlWriter output = new TomlWriter().put("version", 1);
    for (MavenCoordinate root : roots) {
      output.appendTable("roots", coordinateFields(root));
    }
    for (LockedPackage item : packages) {
      Map<String, String> fields = coordinateFields(item.coordinate());
      fields.put("sha256", item.sha256());
      output.appendTable("package", fields);
    }
    Files.createDirectories(file.toAbsolutePath().getParent());
    Files.writeString(file, output.render(), StandardCharsets.UTF_8);
  }

  public void verify(Iterable<String> declaredRoots, List<MavenResolver.ResolvedArtifact> resolved)
      throws Exception {
    Map<String, String> lockedHashes = new LinkedHashMap<>();
    for (LockedPackage item : packages)
      lockedHashes.put(item.coordinate().notation(), item.sha256());
    for (String declared : declaredRoots) {
      String notation = MavenCoordinate.parse(declared).notation();
      if (roots.stream().noneMatch(root -> root.notation().equals(notation))) {
        throw new IOException(
            "jot.lock is out of date: missing root " + notation + "; run `jot lock`");
      }
    }
    for (MavenResolver.ResolvedArtifact artifact : resolved) {
      String notation = artifact.coordinate().notation();
      String expected = lockedHashes.get(notation);
      if (expected == null) {
        throw new IOException(
            "jot.lock is out of date: missing package " + notation + "; run `jot lock`");
      }
      String actual =
          HexFormat.of()
              .formatHex(
                  MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(artifact.jar())));
      if (!expected.equalsIgnoreCase(actual)) {
        throw new IOException("jot.lock checksum mismatch for " + notation);
      }
    }
  }

  private static MavenCoordinate coordinate(Map<String, Object> fields, Path file)
      throws IOException {
    String group = string(fields, "group");
    String artifact = string(fields, "artifact");
    String version = string(fields, "version");
    if (group == null || artifact == null || version == null) {
      throw new IOException("incomplete coordinate in " + file);
    }
    return new MavenCoordinate(
        group,
        artifact,
        version,
        java.util.Objects.requireNonNullElse(string(fields, "classifier"), ""));
  }

  private static String string(Map<String, Object> fields, String key) {
    Object value = fields.get(key);
    return value instanceof String string ? string : null;
  }

  private static Map<String, String> coordinateFields(MavenCoordinate coordinate) {
    Map<String, String> fields = new LinkedHashMap<>();
    fields.put("group", coordinate.group());
    fields.put("artifact", coordinate.artifact());
    fields.put("version", coordinate.version());
    if (!coordinate.classifier().isEmpty()) {
      fields.put("classifier", coordinate.classifier());
    }
    return fields;
  }

  public record LockedPackage(MavenCoordinate coordinate, String sha256) {}
}
