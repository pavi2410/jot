package dev.jot.core.config;

import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.List;

/** Edits dependency entries while preserving the rest of a project's manifest text. */
public final class ManifestEditor {
  private ManifestEditor() {}

  public static void addDependency(Path manifest, String name, String coordinate, boolean test)
      throws IOException {
    addDependencyValue(manifest, name, coordinate, test);
  }

  public static void addCatalogDependency(
      Path manifest, String name, String catalogAlias, boolean test) throws IOException {
    addDependencyValue(manifest, name, java.util.Map.of("catalog", catalogAlias), test);
  }

  public static void pinJavaToolchain(Path manifest, String version) throws IOException {
    TomlDocument.read(manifest);
    List<String> lines = new ArrayList<>(Files.readAllLines(manifest));
    removeEntry(lines, "toolchains", "java");
    insertEntry(lines, "toolchains", "java", version);
    writeValidated(manifest, String.join("\n", lines) + "\n");
  }

  private static void addDependencyValue(Path manifest, String name, Object value, boolean test)
      throws IOException {
    if (name == null || name.isBlank())
      throw new IllegalArgumentException("dependency name cannot be blank");
    TomlDocument.read(manifest);
    String section = test ? "test-dependencies" : "dependencies";
    List<String> lines = new ArrayList<>(Files.readAllLines(manifest));
    removeEntry(lines, section, name);
    insertEntry(lines, section, name, value);
    writeValidated(manifest, String.join("\n", lines) + "\n");
  }

  public static boolean removeDependency(Path manifest, String name, boolean test)
      throws IOException {
    TomlDocument document = TomlDocument.read(manifest);
    String section = test ? "test-dependencies" : "dependencies";
    if (!document.table(section).containsKey(name)) return false;
    List<String> lines = new ArrayList<>(Files.readAllLines(manifest));
    if (!removeEntry(lines, section, name)) {
      throw new IOException("cannot safely locate dependency `" + name + "` in " + manifest);
    }
    writeValidated(manifest, String.join("\n", lines) + "\n");
    return true;
  }

  private static boolean removeEntry(List<String> lines, String section, String name) {
    int sectionStart = findSection(lines, section);
    if (sectionStart >= 0) {
      int sectionEnd = nextTable(lines, sectionStart + 1);
      String quoted = TomlWriter.renderKey(name);
      for (int i = sectionStart + 1; i < sectionEnd; i++) {
        String key = assignmentKey(lines.get(i));
        if (key != null && (key.equals(quoted) || key.equals(name))) {
          lines.remove(i);
          return true;
        }
      }
    }

    String nestedHeader = "[" + section + "." + TomlWriter.renderKey(name) + "]";
    for (int i = 0; i < lines.size(); i++) {
      if (!lines.get(i).trim().equals(nestedHeader)) continue;
      int end = nextTable(lines, i + 1);
      lines.subList(i, end).clear();
      return true;
    }
    return false;
  }

  private static void insertEntry(List<String> lines, String section, String name, Object value) {
    String entry = TomlWriter.renderKey(name) + " = " + TomlWriter.renderValue(value);
    int sectionStart = findSection(lines, section);
    if (sectionStart < 0) {
      while (!lines.isEmpty() && lines.getLast().isBlank()) lines.removeLast();
      if (!lines.isEmpty()) lines.add("");
      lines.add("[" + section + "]");
      lines.add(entry);
      return;
    }

    int insertion = nextTable(lines, sectionStart + 1);
    while (insertion > sectionStart + 1 && lines.get(insertion - 1).isBlank()) insertion--;
    lines.add(insertion, entry);
  }

  private static int findSection(List<String> lines, String section) {
    String header = "[" + section + "]";
    for (int i = 0; i < lines.size(); i++) {
      String line = lines.get(i).stripLeading();
      if (line.equals(header) || line.startsWith(header + " #")) return i;
    }
    return -1;
  }

  private static int nextTable(List<String> lines, int start) {
    for (int i = start; i < lines.size(); i++) {
      String line = lines.get(i).stripLeading();
      if (line.startsWith("[") && line.matches("\\[\\[?.*?\\]\\]?\\s*(?:#.*)?")) return i;
    }
    return lines.size();
  }

  private static String assignmentKey(String line) {
    String trimmed = line.stripLeading();
    if (trimmed.startsWith("#")) return null;
    int equals = assignmentSeparator(trimmed);
    return equals < 1 ? null : trimmed.substring(0, equals).trim();
  }

  private static int assignmentSeparator(String line) {
    char quote = 0;
    boolean escaped = false;
    for (int i = 0; i < line.length(); i++) {
      char character = line.charAt(i);
      if (escaped) {
        escaped = false;
      } else if (quote == '"' && character == '\\') {
        escaped = true;
      } else if (quote != 0 && character == quote) {
        quote = 0;
      } else if (quote == 0 && (character == '"' || character == '\'')) {
        quote = character;
      } else if (quote == 0 && character == '=') {
        return i;
      }
    }
    return -1;
  }

  private static void writeValidated(Path manifest, String contents) throws IOException {
    Path parent = manifest.toAbsolutePath().getParent();
    Path temporary = Files.createTempFile(parent, ".jot-manifest-", ".toml");
    try {
      Files.writeString(temporary, contents);
      TomlDocument.read(temporary);
      try {
        Files.move(
            temporary,
            manifest,
            StandardCopyOption.ATOMIC_MOVE,
            StandardCopyOption.REPLACE_EXISTING);
      } catch (AtomicMoveNotSupportedException ignored) {
        Files.move(temporary, manifest, StandardCopyOption.REPLACE_EXISTING);
      }
    } finally {
      Files.deleteIfExists(temporary);
    }
  }
}
