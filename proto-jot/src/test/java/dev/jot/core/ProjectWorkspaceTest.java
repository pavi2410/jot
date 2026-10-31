package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ProjectWorkspaceTest {
    @TempDir Path temp;

    @Test
    void ordersModulesByLocalDependenciesAndInheritsTheJavaToolchain() throws Exception {
        write("jot.toml", """
                [workspace]
                members = ["app", "core"]
                [toolchains]
                java = "27"
                """);
        write("core/jot.toml", """
                [project]
                name = "jot-core"
                """);
        write("app/jot.toml", """
                [project]
                name = "jot-app"
                main-class = "sample.Main"
                [dependencies]
                core = { path = "../core" }
                """);

        ProjectConfig root = ProjectConfig.read(temp.resolve("jot.toml"), null);
        ProjectWorkspace workspace = ProjectWorkspace.load(temp, root);

        assertEquals(List.of("jot-core", "jot-app"),
                workspace.ordered().stream().map(WorkspaceProject::name).toList());
        assertEquals("27", workspace.find("app").config().javaVersion());
        assertEquals("jot-core", workspace.find("app").dependencies().getFirst().name());
        assertEquals("jot-app", workspace.selectRunnable("app").name());
    }

    @Test
    void reportsCyclesInLocalDependencies() throws Exception {
        write("jot.toml", """
                [workspace]
                members = ["alpha", "beta"]
                [toolchains]
                java = "27"
                """);
        write("alpha/jot.toml", """
                [project]
                name = "alpha"
                [dependencies]
                beta = { path = "../beta" }
                """);
        write("beta/jot.toml", """
                [project]
                name = "beta"
                [dependencies]
                alpha = { path = "../alpha" }
                """);

        ProjectConfig root = ProjectConfig.read(temp.resolve("jot.toml"), null);
        IllegalArgumentException error = assertThrows(IllegalArgumentException.class,
                () -> ProjectWorkspace.load(temp, root));
        org.junit.jupiter.api.Assertions.assertTrue(error.getMessage().contains("cycle"));
    }

    private void write(String relativePath, String contents) throws Exception {
        Path file = temp.resolve(relativePath);
        Files.createDirectories(file.getParent());
        Files.writeString(file, contents);
    }
}
