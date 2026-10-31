package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.jot.core.resolve.Lockfile;
import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.FileTime;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;
import java.util.jar.JarEntry;
import java.util.jar.JarOutputStream;
import java.util.zip.ZipFile;
import javax.tools.ToolProvider;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class BuildEngineTest {
  @TempDir Path temp;

  @Test
  void skipsJmhResolutionWhenNoBenchmarkSourcesExist() throws Exception {
    write(
        "src/sample/App.java",
        "package sample; public final class App { public static void main(String[] args) {} }\n");
    write(
        "jot.toml",
        """
        [project]
        name = "no-benchmarks"
        main-class = "sample.App"
        [toolchains]
        java = "27"
        """);
    ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
    WorkspaceProject project = new WorkspaceProject(temp, config, List.of());
    MavenResolver resolver =
        new MavenResolver(temp.resolve("cache"), MavenResolver.defaultRepository(), true);

    assertFalse(BuildEngine.bench(project, resolver, null, 1, null, null));
    assertTrue(Files.isRegularFile(temp.resolve("target/no-benchmarks-0.1.0.jar")));
  }

  @Test
  void generatesJavadocsForConfiguredSources() throws Exception {
    write(
        "src/sample/PublicApi.java",
        """
        package sample;
        /** A public API for documentation generation. */
        public final class PublicApi {
            /** Creates the API instance. */
            public PublicApi() {}
            /**
             * Returns the greeting.
             * @return the greeting text
             */
            public String greeting() { return "hello"; }
        }
        """);
    write(
        "jot.toml",
        """
        [project]
        name = "documented"
        [toolchains]
        java = "27"
        """);
    String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
    Process process =
        new ProcessBuilder(
                java,
                "-Duser.home=" + temp.resolve("home"),
                "-cp",
                System.getProperty("java.class.path"),
                "dev.jot.cli.Main",
                "doc",
                "--project",
                temp.toString())
            .directory(temp.toFile())
            .redirectErrorStream(true)
            .start();
    String output = new String(process.getInputStream().readAllBytes());

    assertTrue(process.waitFor() == 0, output);
    assertTrue(Files.isRegularFile(temp.resolve("target/docs/index.html")));
    assertTrue(Files.isRegularFile(temp.resolve("target/docs/sample/PublicApi.html")));
  }

  @Test
  void reportsJavacLintWarningsAndRemovesItsTemporaryClasses() throws Exception {
    write(
        "src/sample/LintSample.java",
        """
        package sample;
        import java.util.List;
        public final class LintSample {
            public List values() { return List.of("jot"); }
        }
        """);
    write(
        "jot.toml",
        """
        [project]
        name = "lint-sample"
        [toolchains]
        java = "27"
        """);
    ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
    WorkspaceProject project = new WorkspaceProject(temp, config, List.of());
    MavenResolver resolver =
        new MavenResolver(temp.resolve("cache"), MavenResolver.defaultRepository());

    BuildEngine.LintResult warnings = BuildEngine.lint(project, resolver);

    assertTrue(!warnings.clean());
    assertTrue(warnings.diagnostics().contains("[rawtypes]"), warnings.diagnostics());
    try (var files = Files.list(temp.resolve("target"))) {
      assertTrue(files.noneMatch(path -> path.getFileName().toString().startsWith(".jot-lint-")));
    }

    write(
        "src/sample/LintSample.java",
        """
        package sample;
        import java.util.List;
        public final class LintSample {
            public List<String> values() { return List.of("jot"); }
        }
        """);
    assertTrue(BuildEngine.lint(project, resolver).clean());
  }

  @Test
  void formatsConfiguredMainAndTestSourcesAndChecksWithoutWriting() throws Exception {
    MavenCoordinate formatter = MavenCoordinate.parse("sample:formatter:1.0.0");
    Path repository = temp.resolve("formatter-repository");
    Path formatterSource = temp.resolve("formatter-src/com/google/googlejavaformat/java/Main.java");
    Files.createDirectories(formatterSource.getParent());
    Files.writeString(
        formatterSource,
        """
        package com.google.googlejavaformat.java;
        import java.nio.file.Files;
        import java.nio.file.Path;
        import java.util.Arrays;
        public final class Main {
            public static void main(String[] args) throws Exception {
                boolean check = Arrays.asList(args).contains("--dry-run");
                for (String argument : args) {
                    if (!argument.endsWith(".java")) continue;
                    Path file = Path.of(argument);
                    if (check && !Files.readString(file).equals("class Sample {}\\n")) {
                        System.err.println("Files are not formatted");
                        System.exit(1);
                    }
                    if (!check) Files.writeString(file, "class Sample {}\\n");
                }
            }
        }
        """);
    Path formatterClasses = temp.resolve("formatter-classes");
    Files.createDirectories(formatterClasses);
    assertEquals(
        0,
        ToolProvider.getSystemJavaCompiler()
            .run(
                null,
                null,
                null,
                "--release",
                "27",
                "-d",
                formatterClasses.toString(),
                formatterSource.toString()));
    Path formatterJar = repository.resolve(formatter.repositoryPath("jar"));
    Files.createDirectories(formatterJar.getParent());
    try (JarOutputStream jar = new JarOutputStream(Files.newOutputStream(formatterJar));
        var classes = Files.walk(formatterClasses)) {
      for (Path file : classes.filter(Files::isRegularFile).toList()) {
        jar.putNextEntry(
            new JarEntry(
                formatterClasses
                    .relativize(file)
                    .toString()
                    .replace(java.io.File.separatorChar, '/')));
        Files.copy(file, jar);
        jar.closeEntry();
      }
    }
    Path formatterPom = repository.resolve(formatter.pomPath());
    Files.createDirectories(formatterPom.getParent());
    Files.writeString(
        formatterPom,
        "<project><groupId>sample</groupId>"
            + "<artifactId>formatter</artifactId><version>1.0.0</version></project>");

    write("src/main/java/sample/Sample.java", "class Sample{}\n");
    write("src/test/java/sample/SampleTest.java", "class SampleTest{}\n");
    write(
        "jot.toml",
        """
        [project]
        name = "format-sample"
        layout = "maven"
        [toolchains]
        java = "27"
        [format]
        java = "sample:formatter:1.0.0"
        style = "google"
        """);
    ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
    WorkspaceProject project = new WorkspaceProject(temp, config, List.of());
    MavenResolver resolver = new MavenResolver(temp.resolve("formatter-cache"), repository.toUri());
    ProjectWorkspace workspace = ProjectWorkspace.loadProject(temp, config);

    BuildEngine.lock(workspace, temp.resolve("jot.lock"), resolver);
    assertTrue(
        Lockfile.read(temp.resolve("jot.lock")).roots().stream()
            .anyMatch(root -> root.notation().equals(formatter.notation())));

    BuildEngine.FormatResult needsFormatting = BuildEngine.format(project, resolver, true);
    assertFalse(needsFormatting.formatted());
    BuildEngine.format(project, resolver, false);
    assertEquals(
        "class Sample {}\n", Files.readString(temp.resolve("src/main/java/sample/Sample.java")));
    assertEquals(
        "class Sample {}\n",
        Files.readString(temp.resolve("src/test/java/sample/SampleTest.java")));
    assertTrue(BuildEngine.format(project, resolver, true).formatted());
  }

  @Test
  void producesRepeatableJarAndRebuildsAfterSourceChanges() throws Exception {
    Files.createDirectories(temp.resolve("src/main/java/sample"));
    Path source = temp.resolve("src/main/java/sample/Main.java");
    Files.writeString(
        source,
        "package sample; public class Main { public static String value() { return \"one\"; } }");
    Files.writeString(
        temp.resolve("jot.toml"),
        """
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
    assertArrayEquals(
        firstBuild, repeatBuild, "unchanged inputs should produce identical JAR bytes");

    Path mainClass = temp.resolve("target/classes/sample/Main.class");
    Files.delete(mainClass);
    BuildEngine.build(project);
    assertTrue(Files.isRegularFile(mainClass), "missing classes should be recompiled");
    assertArrayEquals(repeatBuild, Files.readAllBytes(artifact));

    Files.writeString(artifact, "damaged artifact");
    BuildEngine.build(project);
    assertArrayEquals(
        repeatBuild, Files.readAllBytes(artifact), "a damaged JAR should be repackaged");

    Files.writeString(
        source,
        "package sample; public class Main { public static String value() { return \"two\"; } }");
    BuildEngine.build(project);
    assertTrue(
        !HexFormat.of()
            .formatHex(MessageDigest.getInstance("SHA-256").digest(firstBuild))
            .equals(
                HexFormat.of()
                    .formatHex(
                        MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(artifact)))),
        "changed source should change the artifact");
  }

  @Test
  void buildsAndRunsWithTransitiveLocalDependencies() throws Exception {
    write(
        "jot.toml",
        """
        [workspace]
        members = ["app", "core", "base"]
        [toolchains]
        java = "27"
        """);
    write(
        "base/jot.toml",
        """
        [project]
        name = "base"
        [build]
        source-dirs = ["src/main/java"]
        """);
    write(
        "base/src/main/java/sample/base/Base.java",
        """
        package sample.base;
        public final class Base {
            public static String message() { return "transitive-ok"; }
        }
        """);
    write(
        "core/jot.toml",
        """
        [project]
        name = "core"
        [dependencies]
        base = { path = "../base" }
        [build]
        source-dirs = ["src/main/java"]
        """);
    write(
        "core/src/main/java/sample/core/Core.java",
        """
        package sample.core;
        import sample.base.Base;
        public final class Core {
            public static String message() { return Base.message(); }
        }
        """);
    write(
        "app/jot.toml",
        """
        [project]
        name = "app"
        main-class = "sample.app.Main"
        [dependencies]
        core = { path = "../core" }
        [build]
        source-dirs = ["src/main/java"]
        """);
    write(
        "app/src/main/java/sample/app/Main.java",
        """
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
    Files.writeString(
        dependencySource,
        """
        package sample.external;
        public final class External {
            public static String message() { return "maven-ok"; }
        }
        """);
    Path dependencyClasses = temp.resolve("dependency-classes");
    Files.createDirectories(dependencyClasses);
    int compileStatus =
        ToolProvider.getSystemJavaCompiler()
            .run(
                null,
                null,
                null,
                "--release",
                "27",
                "-d",
                dependencyClasses.toString(),
                dependencySource.toString());
    assertEquals(0, compileStatus);
    Path dependencyJar = repository.resolve(external.repositoryPath("jar"));
    Files.createDirectories(dependencyJar.getParent());
    try (JarOutputStream jar = new JarOutputStream(Files.newOutputStream(dependencyJar));
        var classes = Files.walk(dependencyClasses)) {
      for (Path file : classes.filter(Files::isRegularFile).toList()) {
        String entryName =
            dependencyClasses.relativize(file).toString().replace(java.io.File.separatorChar, '/');
        jar.putNextEntry(new JarEntry(entryName));
        Files.copy(file, jar);
        jar.closeEntry();
      }
    }
    Path dependencyPom = repository.resolve(external.pomPath());
    Files.createDirectories(dependencyPom.getParent());
    Files.writeString(
        dependencyPom,
        """
        <project xmlns="http://maven.apache.org/POM/4.0.0">
          <modelVersion>4.0.0</modelVersion>
          <groupId>sample</groupId><artifactId>external-api</artifactId><version>1.0.0</version>
        </project>
        """);

    Files.createDirectories(temp.resolve("src/main/java/sample"));
    Files.writeString(
        temp.resolve("src/main/java/sample/Main.java"),
        """
        package sample;
        import sample.external.External;
        public final class Main {
            public static void main(String[] args) { System.out.println(External.message()); }
        }
        """);
    Files.writeString(
        temp.resolve("jot.toml"),
        """
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
    write(
        "jot.toml",
        """
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
    Path compiledClass = temp.resolve("target/classes/sample/Main.class");
    FileTime unchangedClassTimestamp = FileTime.fromMillis(1_234_567_890_000L);
    Files.setLastModifiedTime(compiledClass, unchangedClassTimestamp);
    try (ZipFile jar = new ZipFile(artifact.toFile())) {
      assertEquals(
          "mode=first\n",
          new String(jar.getInputStream(jar.getEntry("config/app.properties")).readAllBytes()));
    }

    write("assets/config/app.properties", "mode=second\n");
    BuildEngine.build(project);
    byte[] secondBuild = Files.readAllBytes(artifact);
    assertEquals(
        unchangedClassTimestamp,
        Files.getLastModifiedTime(compiledClass),
        "resource-only changes should skip javac");
    assertTrue(
        !java.util.Arrays.equals(firstBuild, secondBuild),
        "resource changes should invalidate the build fingerprint");
    try (ZipFile jar = new ZipFile(artifact.toFile())) {
      assertEquals(
          "mode=second\n",
          new String(jar.getInputStream(jar.getEntry("config/app.properties")).readAllBytes()));
    }

    Files.delete(temp.resolve("assets/config/app.properties"));
    BuildEngine.build(project);
    assertEquals(
        unchangedClassTimestamp,
        Files.getLastModifiedTime(compiledClass),
        "resource deletion should skip javac");
    try (ZipFile jar = new ZipFile(artifact.toFile())) {
      assertNull(
          jar.getEntry("config/app.properties"),
          "deleted resources should disappear from the rebuilt JAR");
    }
    byte[] withoutResource = Files.readAllBytes(artifact);
    BuildEngine.build(project);
    assertEquals(
        unchangedClassTimestamp,
        Files.getLastModifiedTime(compiledClass),
        "an unchanged project should skip javac");
    assertArrayEquals(
        withoutResource,
        Files.readAllBytes(artifact),
        "an unchanged project should keep its artifact");
  }

  @Test
  void incrementallyCompilesApiStableChangesAndFullyRebuildsForApiChanges() throws Exception {
    write(
        "src/sample/Library.java",
        """
        package sample;
        public final class Library {
            public static String message() { return "before"; }
        }
        """);
    write(
        "src/sample/Main.java",
        """
        package sample;
        public final class Main {
            public static String message() { return Library.message(); }
        }
        """);
    write(
        "jot.toml",
        """
        [project]
        name = "incremental-app"
        [toolchains]
        java = "27"
        """);
    ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
    WorkspaceProject project = new WorkspaceProject(temp, config, List.of());
    BuildEngine.build(project);

    Path mainClass = temp.resolve("target/classes/sample/Main.class");
    Path libraryClass = temp.resolve("target/classes/sample/Library.class");
    FileTime sentinel = FileTime.fromMillis(1_234_567_890_000L);
    Files.setLastModifiedTime(mainClass, sentinel);
    byte[] oldLibraryClass = Files.readAllBytes(libraryClass);
    write(
        "src/sample/Library.java",
        """
        package sample;
        public final class Library {
            public static String message() { return "after"; }
        }
        """);

    BuildEngine.build(project);

    assertEquals(
        sentinel,
        Files.getLastModifiedTime(mainClass),
        "an API-stable edit should leave dependent classes untouched");
    assertTrue(!java.util.Arrays.equals(oldLibraryClass, Files.readAllBytes(libraryClass)));
    try (URLClassLoader loader =
        new URLClassLoader(
            new java.net.URL[] {temp.resolve("target/classes").toUri().toURL()}, null)) {
      assertEquals("after", loader.loadClass("sample.Library").getMethod("message").invoke(null));
    }

    Files.setLastModifiedTime(mainClass, sentinel);
    write(
        "src/sample/Library.java",
        """
        package sample;
        public final class Library {
            public static String greeting() { return "api-changed"; }
        }
        """);
    write(
        "src/sample/Main.java",
        """
        package sample;
        public final class Main {
            public static String message() { return Library.greeting(); }
        }
        """);

    BuildEngine.build(project);

    assertNotEquals(
        sentinel,
        Files.getLastModifiedTime(mainClass),
        "an API change should recompile dependent project sources");

    Files.setLastModifiedTime(mainClass, sentinel);
    write("src/sample/Extra.java", "package sample; public final class Extra {}\n");
    BuildEngine.build(project);
    assertTrue(Files.isRegularFile(temp.resolve("target/classes/sample/Extra.class")));
    assertNotEquals(
        sentinel,
        Files.getLastModifiedTime(mainClass),
        "adding a source should rebuild the complete source set");

    Files.setLastModifiedTime(mainClass, sentinel);
    Files.delete(temp.resolve("src/sample/Extra.java"));
    BuildEngine.build(project);
    assertTrue(!Files.exists(temp.resolve("target/classes/sample/Extra.class")));
    assertNotEquals(
        sentinel,
        Files.getLastModifiedTime(mainClass),
        "deleting a source should remove stale classes in a full rebuild");
  }

  private void write(String relativePath, String contents) throws Exception {
    Path file = temp.resolve(relativePath);
    Files.createDirectories(file.getParent());
    Files.writeString(file, contents);
  }
}
