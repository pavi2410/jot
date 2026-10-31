package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.assertFalse;

import dev.jot.core.resolve.Lockfile;
import dev.jot.core.resolve.MavenCoordinate;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class DependencyListingTest {
    @TempDir Path temp;

    @Test
    void listsModuleScopesLocalDependenciesAndLockStatus() throws Exception {
        write("jot.toml", """
                [workspace]
                members = ["app", "core"]
                [toolchains]
                java = "27"
                """);
        write("core/jot.toml", """
                [project]
                name = "core"
                """);
        write("app/jot.toml", """
                [project]
                name = "app"
                [dependencies]
                core = { path = "../core" }
                json = "org.example:json:1.2.0"
                [test-dependencies]
                junit = "org.junit.jupiter:junit-jupiter:5.11.0"
                """);

        ProjectConfig root = ProjectConfig.read(temp.resolve("jot.toml"), null);
        ProjectWorkspace workspace = ProjectWorkspace.load(temp, root);
        Path lock = temp.resolve("jot.lock");
        new Lockfile(List.of(MavenCoordinate.parse("org.example:json:1.2.0")), List.of())
                .write(lock);

        String output = DependencyListing.render(root, workspace, "app", lock);

        assertTrue(output.contains("app:"));
        assertTrue(output.contains("main  json = org.example:json:1.2.0 [locked]"));
        assertTrue(output.contains("path  core = ../core [local]"));
        assertTrue(output.contains("test  junit = org.junit.jupiter:junit-jupiter:5.11.0 [unlocked]"));
        assertFalse(output.contains("core:\n"));
    }

    private void write(String relativePath, String contents) throws Exception {
        Path file = temp.resolve(relativePath);
        Files.createDirectories(file.getParent());
        Files.writeString(file, contents);
    }
}
