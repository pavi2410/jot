package dev.jot.core;

import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;
import java.util.zip.ZipFile;
import java.util.jar.JarEntry;
import java.util.jar.JarOutputStream;
import javax.tools.ToolProvider;
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

        Path mainClass = temp.resolve("target/classes/sample/Main.class");
        Files.delete(mainClass);
        BuildEngine.build(project);
        assertTrue(Files.isRegularFile(mainClass), "missing classes should be recompiled");
        assertArrayEquals(repeatBuild, Files.readAllBytes(artifact));

        Files.writeString(artifact, "damaged artifact");
        BuildEngine.build(project);
        assertArrayEquals(repeatBuild, Files.readAllBytes(artifact),
                "a damaged JAR should be repackaged");

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

    @Test
    void compilesAndRunsWithAResolvedMavenArtifact() throws Exception {
        Path repository = temp.resolve("repository");
        MavenCoordinate external = MavenCoordinate.parse("sample:external-api:1.0.0");
        Path dependencySource = temp.resolve("dependency-src/sample/external/External.java");
        Files.createDirectories(dependencySource.getParent());
        Files.writeString(dependencySource, """
                package sample.external;
                public final class External {
                    public static String message() { return "maven-ok"; }
                }
                """);
        Path dependencyClasses = temp.resolve("dependency-classes");
        Files.createDirectories(dependencyClasses);
        int compileStatus = ToolProvider.getSystemJavaCompiler().run(null, null, null,
                "--release", "27", "-d", dependencyClasses.toString(), dependencySource.toString());
        assertEquals(0, compileStatus);
        Path dependencyJar = repository.resolve(external.repositoryPath("jar"));
        Files.createDirectories(dependencyJar.getParent());
        try (JarOutputStream jar = new JarOutputStream(Files.newOutputStream(dependencyJar));
             var classes = Files.walk(dependencyClasses)) {
            for (Path file : classes.filter(Files::isRegularFile).toList()) {
                String entryName = dependencyClasses.relativize(file).toString()
                        .replace(java.io.File.separatorChar, '/');
                jar.putNextEntry(new JarEntry(entryName));
                Files.copy(file, jar);
                jar.closeEntry();
            }
        }
        Path dependencyPom = repository.resolve(external.pomPath());
        Files.createDirectories(dependencyPom.getParent());
        Files.writeString(dependencyPom, """
                <project xmlns="http://maven.apache.org/POM/4.0.0">
                  <modelVersion>4.0.0</modelVersion>
                  <groupId>sample</groupId><artifactId>external-api</artifactId><version>1.0.0</version>
                </project>
                """);

        Files.createDirectories(temp.resolve("src/main/java/sample"));
        Files.writeString(temp.resolve("src/main/java/sample/Main.java"), """
                package sample;
                import sample.external.External;
                public final class Main {
                    public static void main(String[] args) { System.out.println(External.message()); }
                }
                """);
        Files.writeString(temp.resolve("jot.toml"), """
                [project]
                name = "with-external"
                version = "1.0.0"
                main-class = "sample.Main"
                [toolchains]
                java = "27"
                [dependencies]
                external = "sample:external-api:1.0.0"
                """);
        ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
        WorkspaceProject project = new WorkspaceProject(temp, config, List.of());
        MavenResolver resolver = new MavenResolver(temp.resolve("cache"), repository.toUri());

        BuildEngine.build(project, resolver);
        BuildEngine.run(project, List.of(), resolver);

        assertTrue(Files.isRegularFile(temp.resolve("target/with-external-1.0.0.jar")));
    }

    @Test
    void packagesConfiguredResourcesAndRebuildsWhenTheyChange() throws Exception {
        write("src/main/java/sample/Main.java", "package sample; public class Main {}\n");
        write("assets/config/app.properties", "mode=first\n");
        write("jot.toml", """
                [project]
                name = "resource-app"
                version = "1.0.0"
                layout = "maven"
                [toolchains]
                java = "27"
                [build]
                source-dirs = ["src/main/java"]
                resource-dirs = ["assets"]
                """);
        ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
        WorkspaceProject project = new WorkspaceProject(temp, config, List.of());
        Path artifact = temp.resolve("target/resource-app-1.0.0.jar");

        BuildEngine.build(project);
        byte[] firstBuild = Files.readAllBytes(artifact);
        try (ZipFile jar = new ZipFile(artifact.toFile())) {
            assertEquals("mode=first\n", new String(jar.getInputStream(
                    jar.getEntry("config/app.properties")).readAllBytes()));
        }

        write("assets/config/app.properties", "mode=second\n");
        BuildEngine.build(project);
        byte[] secondBuild = Files.readAllBytes(artifact);
        assertTrue(!java.util.Arrays.equals(firstBuild, secondBuild),
                "resource changes should invalidate the build fingerprint");
        try (ZipFile jar = new ZipFile(artifact.toFile())) {
            assertEquals("mode=second\n", new String(jar.getInputStream(
                    jar.getEntry("config/app.properties")).readAllBytes()));
        }

        Files.delete(temp.resolve("assets/config/app.properties"));
        BuildEngine.build(project);
        try (ZipFile jar = new ZipFile(artifact.toFile())) {
            assertNull(jar.getEntry("config/app.properties"),
                    "deleted resources should disappear from the rebuilt JAR");
        }
    }

    private void write(String relativePath, String contents) throws Exception {
        Path file = temp.resolve(relativePath);
        Files.createDirectories(file.getParent());
        Files.writeString(file, contents);
    }
}
