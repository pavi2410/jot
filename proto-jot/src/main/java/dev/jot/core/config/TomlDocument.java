package dev.jot.core.config;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** Small, isolated TOML reader used during Java Jot's dependency-free bootstrap. */
public final class TomlDocument {
    private final Path source;
    private final Map<String, Object> values;

    @SuppressWarnings("unchecked")
    private TomlDocument(Path source, Map<String, Object> values) {
        this.source = source;
        Map<String, Object> frozen = new TreeMap<>();
        values.forEach((key, value) -> {
            if (value instanceof List<?> list && list.stream().allMatch(Map.class::isInstance)) {
                List<Map<String, Object>> tables = new ArrayList<>();
                for (Object item : list) tables.add(Map.copyOf((Map<String, Object>) item));
                frozen.put(key, List.copyOf(tables));
            } else {
                frozen.put(key, value);
            }
        });
        this.values = Map.copyOf(frozen);
    }

    public static TomlDocument read(Path source) throws IOException {
        Map<String, Object> values = new TreeMap<>();
        String section = "";
        Map<String, Object> currentArrayTable = null;
        int lineNumber = 0;
        for (String raw : Files.readAllLines(source)) {
            lineNumber++;
            String line = stripComment(raw).trim();
            if (line.isEmpty()) continue;
            if (line.startsWith("[[") && line.endsWith("]]")) {
                String tableName = line.substring(2, line.length() - 2).trim();
                if (tableName.isEmpty()) throw syntax(source, lineNumber, "empty array table name");
                Object existing = values.get(tableName);
                List<Map<String, Object>> tables;
                if (existing == null) {
                    tables = new ArrayList<>();
                    values.put(tableName, tables);
                } else if (existing instanceof List<?> list
                        && list.stream().allMatch(Map.class::isInstance)) {
                    @SuppressWarnings("unchecked")
                    List<Map<String, Object>> typed = (List<Map<String, Object>>) list;
                    tables = typed;
                } else {
                    throw syntax(source, lineNumber, "conflicting table " + tableName);
                }
                currentArrayTable = new LinkedHashMap<>();
                tables.add(currentArrayTable);
                section = "";
                continue;
            }
            if (line.startsWith("[") && line.endsWith("]")) {
                section = line.substring(1, line.length() - 1).trim();
                if (section.isEmpty()) throw syntax(source, lineNumber, "empty table name");
                currentArrayTable = null;
                continue;
            }
            int equals = line.indexOf('=');
            if (equals < 1) throw syntax(source, lineNumber, "expected key = value");
            String localKey = line.substring(0, equals).trim();
            String key = section.isEmpty() ? localKey : section + "." + localKey;
            Map<String, Object> destination = currentArrayTable == null ? values : currentArrayTable;
            Object previous = destination.putIfAbsent(key, parseValue(line.substring(equals + 1).trim(),
                    source, lineNumber));
            if (previous != null) throw syntax(source, lineNumber, "duplicate key " + key);
        }
        return new TomlDocument(source, values);
    }

    public String string(String key, String fallback) {
        Object value = values.get(key);
        if (value == null) return fallback;
        if (value instanceof String string) return string;
        throw new IllegalArgumentException(key + " must be a string in " + source);
    }

    public int integer(String key, int fallback) {
        Object value = values.get(key);
        if (value == null) return fallback;
        if (value instanceof Integer integer) return integer;
        throw new IllegalArgumentException(key + " must be an integer in " + source);
    }

    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> tableArray(String key) {
        Object value = values.get(key);
        if (value == null) return List.of();
        if (value instanceof List<?> list && list.stream().allMatch(Map.class::isInstance)) {
            return (List<Map<String, Object>>) list;
        }
        throw new IllegalArgumentException(key + " must be an array of tables in " + source);
    }

    @SuppressWarnings("unchecked")
    public List<String> stringList(String key, List<String> fallback) {
        Object value = values.get(key);
        if (value == null) return fallback;
        if (value instanceof List<?>) return (List<String>) value;
        throw new IllegalArgumentException(key + " must be an array of strings in " + source);
    }

    public Map<String, Object> values() {
        return values;
    }

    private static Object parseValue(String input, Path source, int lineNumber) {
        if (input.length() >= 2 && input.startsWith("\"") && input.endsWith("\"")) {
            return parseString(input.substring(1, input.length() - 1), source, lineNumber);
        }
        if (input.startsWith("[") && input.endsWith("]")) {
            String contents = input.substring(1, input.length() - 1).trim();
            if (contents.isEmpty()) return List.of();
            List<String> result = new ArrayList<>();
            for (String item : splitDelimited(contents, source, lineNumber)) {
                Object parsed = parseValue(item.trim(), source, lineNumber);
                if (!(parsed instanceof String string)) {
                    throw syntax(source, lineNumber, "array values must be strings");
                }
                result.add(string);
            }
            return List.copyOf(result);
        }
        if (input.startsWith("{") && input.endsWith("}")) {
            String contents = input.substring(1, input.length() - 1).trim();
            Map<String, String> result = new TreeMap<>();
            if (contents.isEmpty()) return Map.copyOf(result);
            for (String item : splitDelimited(contents, source, lineNumber)) {
                int equals = item.indexOf('=');
                if (equals < 1) throw syntax(source, lineNumber, "invalid inline table entry");
                String key = item.substring(0, equals).trim();
                Object parsed = parseValue(item.substring(equals + 1).trim(), source, lineNumber);
                if (!(parsed instanceof String string)) {
                    throw syntax(source, lineNumber, "inline table values must be strings");
                }
                if (result.putIfAbsent(key, string) != null) {
                    throw syntax(source, lineNumber, "duplicate inline table key " + key);
                }
            }
            return Map.copyOf(result);
        }
        if (input.matches("-?[0-9]+")) {
            try {
                return Integer.parseInt(input);
            } catch (NumberFormatException error) {
                throw syntax(source, lineNumber, "integer is out of range");
            }
        }
        throw syntax(source, lineNumber,
                "bootstrap TOML supports strings, integers, string arrays, inline tables, and table arrays");
    }

    private static String parseString(String value, Path source, int lineNumber) {
        StringBuilder result = new StringBuilder();
        boolean escaped = false;
        for (int i = 0; i < value.length(); i++) {
            char character = value.charAt(i);
            if (!escaped && character == '\\') {
                escaped = true;
                continue;
            }
            if (escaped) {
                switch (character) {
                    case '"', '\\' -> result.append(character);
                    case 'n' -> result.append('\n');
                    case 'r' -> result.append('\r');
                    case 't' -> result.append('\t');
                    default -> throw syntax(source, lineNumber, "unsupported string escape: \\" + character);
                }
                escaped = false;
            } else {
                result.append(character);
            }
        }
        if (escaped) throw syntax(source, lineNumber, "unfinished string escape");
        return result.toString();
    }

    private static List<String> splitDelimited(String contents, Path source, int lineNumber) {
        List<String> parts = new ArrayList<>();
        boolean quoted = false;
        boolean escaped = false;
        int start = 0;
        for (int i = 0; i < contents.length(); i++) {
            char character = contents.charAt(i);
            if (escaped) escaped = false;
            else if (quoted && character == '\\') escaped = true;
            else if (character == '"') quoted = !quoted;
            else if (character == ',' && !quoted) {
                parts.add(contents.substring(start, i).trim());
                start = i + 1;
            }
        }
        if (quoted || escaped) throw syntax(source, lineNumber, "unterminated string value");
        parts.add(contents.substring(start).trim());
        return parts;
    }

    private static String stripComment(String value) {
        boolean quoted = false;
        boolean escaped = false;
        for (int i = 0; i < value.length(); i++) {
            char character = value.charAt(i);
            if (escaped) escaped = false;
            else if (quoted && character == '\\') escaped = true;
            else if (character == '"') quoted = !quoted;
            else if (character == '#' && !quoted) return value.substring(0, i);
        }
        return value;
    }

    private static IllegalArgumentException syntax(Path source, int lineNumber, String message) {
        return new IllegalArgumentException(source + ":" + lineNumber + ": " + message);
    }
}
