package dev.jot.core.config;

import java.io.IOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.tomlj.Toml;
import org.tomlj.TomlArray;
import org.tomlj.TomlParseResult;
import org.tomlj.TomlTable;

/** Typed view over a standards-compliant TOML parse result. */
public final class TomlDocument {
    private final Path source;
    private final TomlParseResult document;

    private TomlDocument(Path source, TomlParseResult document) {
        this.source = source;
        this.document = document;
    }

    public static TomlDocument read(Path source) throws IOException {
        TomlParseResult document = Toml.parse(source);
        if (document.hasErrors()) {
            throw new IOException("invalid TOML in " + source + ": "
                    + String.join("; ", document.errors().stream().map(Object::toString).toList()));
        }
        return new TomlDocument(source, document);
    }

    public String string(String key, String fallback) {
        Object value = document.get(key);
        if (value == null) return fallback;
        if (value instanceof String string) return string;
        throw invalidType(key, "a string");
    }

    public int integer(String key, int fallback) {
        Object value = document.get(key);
        if (value == null) return fallback;
        if (value instanceof Long integer && integer >= Integer.MIN_VALUE
                && integer <= Integer.MAX_VALUE) return integer.intValue();
        throw invalidType(key, "an integer");
    }

    public List<String> stringList(String key, List<String> fallback) {
        TomlArray value = document.getArray(key);
        if (value == null) return fallback;
        List<String> result = new ArrayList<>();
        for (Object item : value.toList()) {
            if (!(item instanceof String string)) throw invalidType(key, "an array of strings");
            result.add(string);
        }
        return List.copyOf(result);
    }

    public Map<String, Object> table(String key) {
        TomlTable value = document.getTable(key);
        if (value == null) {
            if (document.contains(key)) throw invalidType(key, "a table");
            return Map.of();
        }
        return convertMap(value.toMap());
    }

    public Map<String, Object> values() {
        return convertMap(document.toMap());
    }

    public List<Map<String, Object>> tableArray(String key) {
        TomlArray value = document.getArray(key);
        if (value == null) {
            if (document.contains(key)) throw invalidType(key, "an array of tables");
            return List.of();
        }
        List<Map<String, Object>> result = new ArrayList<>();
        for (Object item : value.toList()) {
            if (!(item instanceof TomlTable table)) throw invalidType(key, "an array of tables");
            result.add(convertMap(table.toMap()));
        }
        return List.copyOf(result);
    }

    private Map<String, Object> convertMap(Map<String, Object> source) {
        Map<String, Object> result = new LinkedHashMap<>();
        source.forEach((key, value) -> result.put(key, convert(value)));
        return Map.copyOf(result);
    }

    private Object convert(Object value) {
        if (value instanceof TomlTable table) return convertMap(table.toMap());
        if (value instanceof TomlArray array) {
            return List.copyOf(array.toList().stream().map(this::convert).toList());
        }
        return value;
    }

    private IllegalArgumentException invalidType(String key, String expected) {
        return new IllegalArgumentException(key + " must be " + expected + " in " + source);
    }
}
