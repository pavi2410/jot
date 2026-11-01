package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

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
    write(
        "jot.toml",
        """
        [workspace]
        members = ["app", "core"]
        [toolchains]
        java = "27"
        """);
    write(
        "core/jot.toml",
        """
        [project]
        name = "core"
        """);
    write(
        "app/jot.toml",
        """
        [project]
        name = "app"
        [dependencies]
        core = { path = "../core" }
        json = "org.example:json:1.2.0"
        [test-dependencies]
        junit = "org.junit.jupiter:junit-jupiter:5.11.0"
        [format]
        java = "org.example:formatter:1.0.0"
        """);

    ProjectConfig root = ProjectConfig.read(temp.resolve("jot.toml"), null);
    ProjectWorkspace workspace = ProjectWorkspace.load(temp, root);
    Path lock = temp.resolve("jot.lock");
    new Lockfile(
            List.of(
                MavenCoordinate.parse("org.example:json:1.2.0"),
                MavenCoordinate.parse("org.example:formatter:1.0.0")),
            List.of())
        .write(lock);

    String output = DependencyListing.render(root, workspace, "app", lock);

    assertTrue(output.contains("app:"));
    assertTrue(output.contains("main  json = org.example:json:1.2.0 [locked]"));
    assertTrue(output.contains("path  core = ../core [local]"));
    assertTrue(output.contains("test  junit = org.junit.jupiter:junit-jupiter:5.11.0 [unlocked]"));
    assertTrue(output.contains("tool  java-formatter = org.example:formatter:1.0.0 [locked]"));
    assertFalse(output.contains("core:\n"));
  }

  @Test
  void reportsOutdatedLockedPackagesForTheSelectedWorkspaceModule() throws Exception {
    write(
        "jot.toml",
        """
        [workspace]
        members = ["app", "core"]
        [toolchains]
        java = "27"
        """);
    write("core/jot.toml", "[project]\nname = \"core\"\n");
    write(
        "app/jot.toml",
        """
        [project]
        name = "app"
        [dependencies]
        json = "org.example:json:1.2.0"
        [test-dependencies]
        junit = "org.junit.jupiter:junit-jupiter:5.11.0"
        """);
    write("repository/org/example/json/maven-metadata.xml", metadata("1.3.0"));
    write("repository/org/junit/jupiter/junit-jupiter/maven-metadata.xml", metadata("5.11.0"));
    MavenCoordinate json = MavenCoordinate.parse("org.example:json:1.2.0");
    MavenCoordinate junit = MavenCoordinate.parse("org.junit.jupiter:junit-jupiter:5.11.0");
    Lockfile lock =
        new Lockfile(
            List.of(json, junit),
            List.of(
                new Lockfile.LockedPackage(json, "0".repeat(64)),
                new Lockfile.LockedPackage(junit, "1".repeat(64))));
    Path lockPath = temp.resolve("jot.lock");
    lock.write(lockPath);
    Path home = temp.resolve("home");
    Files.createDirectories(home);
    String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
    ProcessBuilder command =
        new ProcessBuilder(
                java,
                "-Duser.home=" + home,
                "-cp",
                System.getProperty("java.class.path"),
                "dev.jot.cli.Main",
                "outdated",
                "--project",
                temp.toString(),
                "--module",
                "app")
            .directory(temp.toFile())
            .redirectErrorStream(true);
    command
        .environment()
        .put("JOT_MAVEN_REPOSITORY", temp.resolve("repository").toUri().toString());
    Process process = command.start();
    String output = new String(process.getInputStream().readAllBytes());

    assertTrue(process.waitFor() == 0, output);
    assertTrue(output.contains("org.example:json 1.2.0 -> 1.3.0"));
    assertFalse(output.contains("junit-jupiter"));
  }

  private static String metadata(String release) {
    return "<metadata><versioning><release>" + release + "</release></versioning></metadata>";
  }

  private void write(String relativePath, String contents) throws Exception {
    Path file = temp.resolve(relativePath);
    Files.createDirectories(file.getParent());
    Files.writeString(file, contents);
  }
}
