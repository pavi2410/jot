package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class BuildEngineTest {
    @TempDir Path temp;

    @Test
    void producesRepeatableJarAndRebuildsAfterSourceChanges() throws Exception {
        Files.createDirectories(temp.resolve("src/main/java/sample"));
        Path source = temp.resolve("src/main/java/sample/Main.java");
        Files.writeString(source, "package sample; public class Main { public static String value() { return \"one\"; } }");
        Files.writeString(temp.resolve("jot.toml"), """
                [project]
                name = "sample"
                version = "1.0.0"
                main-class = "sample.Main"
                [toolchains]
                java = "27"
                [build]
                source-dirs = ["src/main/java"]
                """);
        ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
        WorkspaceProject project = new WorkspaceProject(temp, config, List.of());

        BuildEngine.build(project);
        Path artifact = temp.resolve("target/sample-1.0.0.jar");
        byte[] firstBuild = Files.readAllBytes(artifact);
        BuildEngine.clean(project);
        BuildEngine.build(project);
        byte[] repeatBuild = Files.readAllBytes(artifact);
        assertArrayEquals(firstBuild, repeatBuild, "unchanged inputs should produce identical JAR bytes");

        Files.writeString(source, "package sample; public class Main { public static String value() { return \"two\"; } }");
        BuildEngine.build(project);
        assertTrue(!HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(firstBuild))
                .equals(HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256")
                        .digest(Files.readAllBytes(artifact)))), "changed source should change the artifact");
    }

    @Test
    void buildsAndRunsWithTransitiveLocalDependencies() throws Exception {
        write("jot.toml", """
                [workspace]
                members = ["app", "core", "base"]
                [toolchains]
                java = "27"
                """);
        write("base/jot.toml", """
                [project]
                name = "base"
                [build]
                source-dirs = ["src/main/java"]
                """);
        write("base/src/main/java/sample/base/Base.java", """
                package sample.base;
                public final class Base {
                    public static String message() { return "transitive-ok"; }
                }
                """);
        write("core/jot.toml", """
                [project]
                name = "core"
                [dependencies]
                base = { path = "../base" }
                [build]
                source-dirs = ["src/main/java"]
                """);
        write("core/src/main/java/sample/core/Core.java", """
                package sample.core;
                import sample.base.Base;
                public final class Core {
                    public static String message() { return Base.message(); }
                }
                """);
        write("app/jot.toml", """
                [project]
                name = "app"
                main-class = "sample.app.Main"
                [dependencies]
                core = { path = "../core" }
                [build]
                source-dirs = ["src/main/java"]
                """);
        write("app/src/main/java/sample/app/Main.java", """
                package sample.app;
                import sample.base.Base;
                public final class Main {
                    public static void main(String[] args) { System.out.println(Base.message()); }
                }
                """);

        ProjectConfig rootConfig = ProjectConfig.read(temp.resolve("jot.toml"), null);
        ProjectWorkspace workspace = ProjectWorkspace.load(temp, rootConfig);
        for (WorkspaceProject project : workspace.ordered()) BuildEngine.build(project);

        BuildEngine.run(workspace.selectRunnable("app"), List.of());
        assertTrue(Files.isRegularFile(temp.resolve("app/target/app-0.1.0.jar")));
    }

    private void write(String relativePath, String contents) throws Exception {
        Path file = temp.resolve(relativePath);
        Files.createDirectories(file.getParent());
        Files.writeString(file, contents);
    }
}
