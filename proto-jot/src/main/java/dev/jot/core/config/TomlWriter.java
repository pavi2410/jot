package dev.jot.core.config;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Small isolated TOML document writer for generated manifests and lockfiles. */
public final class TomlWriter {
  private final Map<String, Object> values = new LinkedHashMap<>();
  private final List<Table> tables = new ArrayList<>();

  public TomlWriter put(String key, String value) {
    values.put(key, value);
    return this;
  }

  public TomlWriter put(String key, int value) {
    values.put(key, value);
    return this;
  }

  public TomlWriter appendTable(String name, Map<String, String> fields) {
    tables.add(new Table(name, new LinkedHashMap<>(fields)));
    return this;
  }

  public String render() {
    StringBuilder output = new StringBuilder();
    for (Map.Entry<String, Object> entry : values.entrySet()) {
      output
          .append(entry.getKey())
          .append(" = ")
          .append(renderValue(entry.getValue()))
          .append('\n');
    }
    for (Table table : tables) {
      if (!output.isEmpty()) output.append('\n');
      output.append("[[").append(table.name()).append("]]\n");
      for (Map.Entry<String, String> field : table.fields().entrySet()) {
        output
            .append(field.getKey())
            .append(" = ")
            .append(renderValue(field.getValue()))
            .append('\n');
      }
    }
    return output.toString();
  }

  public static String renderValue(Object value) {
    if (value instanceof String string) return "\"" + escape(string) + "\"";
    if (value instanceof Integer integer) return integer.toString();
    if (value instanceof Map<?, ?> table) {
      List<String> entries = new ArrayList<>();
      for (Map.Entry<?, ?> entry : table.entrySet()) {
        if (!(entry.getKey() instanceof String key)) {
          throw new IllegalArgumentException("TOML inline table keys must be strings");
        }
        entries.add(renderKey(key) + " = " + renderValue(entry.getValue()));
      }
      return "{ " + String.join(", ", entries) + " }";
    }
    throw new IllegalArgumentException("unsupported TOML value type: " + value.getClass());
  }

  public static String renderKey(String key) {
    if (key.matches("[A-Za-z0-9_-]+")) return key;
    return "\"" + escape(key) + "\"";
  }

  private static String escape(String value) {
    return value
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t");
  }

  private record Table(String name, Map<String, String> fields) {}
}
