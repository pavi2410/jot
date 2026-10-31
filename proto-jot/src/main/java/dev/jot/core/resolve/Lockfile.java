package dev.jot.core.resolve;

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
    public static Lockfile create(Iterable<String> declaredRoots,
                                  List<MavenResolver.ResolvedArtifact> artifacts) throws Exception {
        List<MavenCoordinate> roots = new ArrayList<>();
        for (String root : declaredRoots) roots.add(MavenCoordinate.parse(root));
        List<LockedPackage> packages = new ArrayList<>();
        for (MavenResolver.ResolvedArtifact artifact : artifacts) {
            byte[] bytes = Files.readAllBytes(artifact.jar());
            String checksum = HexFormat.of().formatHex(
                    MessageDigest.getInstance("SHA-256").digest(bytes));
            packages.add(new LockedPackage(artifact.coordinate(), checksum));
        }
        roots.sort(Comparator.comparing(MavenCoordinate::notation));
        packages.sort(Comparator.comparing(item -> item.coordinate().notation()));
        return new Lockfile(List.copyOf(roots), List.copyOf(packages));
    }

    public static Lockfile read(Path file) throws IOException {
        if (!Files.isRegularFile(file)) throw new IOException("lockfile not found: " + file);
        List<Map<String, String>> roots = new ArrayList<>();
        List<Map<String, String>> packages = new ArrayList<>();
        Map<String, String> current = null;
        String section = "";
        int lineNumber = 0;
        for (String raw : Files.readAllLines(file)) {
            lineNumber++;
            String line = raw.strip();
            if (line.isEmpty() || line.startsWith("#")) continue;
            if (line.equals("[[roots]]")) {
                section = "roots";
                current = new LinkedHashMap<>();
                roots.add(current);
                continue;
            }
            if (line.equals("[[package]]")) {
                section = "package";
                current = new LinkedHashMap<>();
                packages.add(current);
                continue;
            }
            int equals = line.indexOf('=');
            if (equals < 1) throw syntax(file, lineNumber, "expected key = value");
            String key = line.substring(0, equals).trim();
            String value = line.substring(equals + 1).trim();
            if (section.isEmpty()) {
                if (!key.equals("version") || !value.equals("1")) {
                    throw syntax(file, lineNumber, "unsupported lockfile header");
                }
                continue;
            }
            if (current == null || value.length() < 2 || !value.startsWith("\"")
                    || !value.endsWith("\"")) {
                throw syntax(file, lineNumber, "lockfile values must be quoted strings");
            }
            current.put(key, unescape(value.substring(1, value.length() - 1), file, lineNumber));
        }
        List<MavenCoordinate> parsedRoots = new ArrayList<>();
        for (Map<String, String> root : roots) parsedRoots.add(coordinate(root, file));
        List<LockedPackage> parsedPackages = new ArrayList<>();
        for (Map<String, String> item : packages) {
            String checksum = item.get("sha256");
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

    public void verify(Iterable<String> declaredRoots,
                       List<MavenResolver.ResolvedArtifact> resolved) throws Exception {
        Map<String, String> lockedHashes = new LinkedHashMap<>();
        for (LockedPackage item : packages) lockedHashes.put(item.coordinate().notation(), item.sha256());
        for (String declared : declaredRoots) {
            String notation = MavenCoordinate.parse(declared).notation();
            if (roots.stream().noneMatch(root -> root.notation().equals(notation))) {
                throw new IOException("jot.lock is out of date: missing root " + notation
                        + "; run `jot lock`");
            }
        }
        for (MavenResolver.ResolvedArtifact artifact : resolved) {
            String notation = artifact.coordinate().notation();
            String expected = lockedHashes.get(notation);
            if (expected == null) {
                throw new IOException("jot.lock is out of date: missing package " + notation
                        + "; run `jot lock`");
            }
            String actual = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256")
                    .digest(Files.readAllBytes(artifact.jar())));
            if (!expected.equalsIgnoreCase(actual)) {
                throw new IOException("jot.lock checksum mismatch for " + notation);
            }
        }
    }

    private static MavenCoordinate coordinate(Map<String, String> fields, Path file) throws IOException {
        String group = fields.get("group");
        String artifact = fields.get("artifact");
        String version = fields.get("version");
        if (group == null || artifact == null || version == null) {
            throw new IOException("incomplete coordinate in " + file);
        }
        return new MavenCoordinate(group, artifact, version, fields.getOrDefault("classifier", ""));
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

    private static String unescape(String value, Path file, int lineNumber) throws IOException {
        StringBuilder result = new StringBuilder();
        boolean escaped = false;
        for (int i = 0; i < value.length(); i++) {
            char character = value.charAt(i);
            if (escaped) {
                if (character != '\\' && character != '"') {
                    throw syntax(file, lineNumber, "invalid string escape");
                }
                result.append(character);
                escaped = false;
            } else if (character == '\\') {
                escaped = true;
            } else {
                result.append(character);
            }
        }
        if (escaped) throw syntax(file, lineNumber, "unfinished string escape");
        return result.toString();
    }

    private static IOException syntax(Path file, int lineNumber, String message) {
        return new IOException(file + ":" + lineNumber + ": " + message);
    }

    public record LockedPackage(MavenCoordinate coordinate, String sha256) {}
}
