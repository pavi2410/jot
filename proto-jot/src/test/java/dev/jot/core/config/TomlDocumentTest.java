package dev.jot.core.config;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class TomlDocumentTest {
    @TempDir Path temp;

    @Test
    void readsQuotedStringsArraysAndPathDependencyTables() throws Exception {
        Path manifest = temp.resolve("jot.toml");
        Files.writeString(manifest, """
                [project]
                name = "Jot # Java" # comment outside the value
                version = "0.2.0"

                [workspace]
                members = ["core", "cli"]

                [dependencies]
                core = { path = "../core,shared" }
                """);

        TomlDocument document = TomlDocument.read(manifest);

        assertEquals("Jot # Java", document.string("project.name", ""));
        assertEquals("0.2.0", document.string("project.version", ""));
        assertEquals(java.util.List.of("core", "cli"),
                document.stringList("workspace.members", java.util.List.of()));
        assertEquals("../core,shared", ((java.util.Map<?, ?>)
                document.values().get("dependencies.core")).get("path"));
    }
}
