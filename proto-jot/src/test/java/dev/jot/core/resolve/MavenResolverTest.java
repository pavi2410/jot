package dev.jot.core.resolve;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.jar.JarOutputStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class MavenResolverTest {
  @TempDir Path temp;

  @Test
  void resolvesPropertiesManagedVersionsTransitivelyAndNearestConflictWinner() throws Exception {
    Path repository = temp.resolve("repository");
    addArtifact(
        repository,
        "sample:api:1.0.0",
        """
        <project xmlns="http://maven.apache.org/POM/4.0.0">
          <modelVersion>4.0.0</modelVersion>
          <groupId>sample</groupId><artifactId>api</artifactId><version>1.0.0</version>
          <properties><util.version>1.2.0</util.version></properties>
          <dependencyManagement><dependencies>
            <dependency><groupId>sample</groupId><artifactId>shared</artifactId><version>1.0.0</version></dependency>
          </dependencies></dependencyManagement>
          <dependencies>
            <dependency><groupId>sample</groupId><artifactId>util</artifactId><version>${util.version}</version></dependency>
            <dependency><groupId>sample</groupId><artifactId>shared</artifactId></dependency>
            <dependency><groupId>sample</groupId><artifactId>test-only</artifactId><scope>test</scope></dependency>
          </dependencies>
        </project>
        """);
    addArtifact(
        repository,
        "sample:util:1.2.0",
        """
        <project xmlns="http://maven.apache.org/POM/4.0.0">
          <modelVersion>4.0.0</modelVersion>
          <groupId>sample</groupId><artifactId>util</artifactId><version>1.2.0</version>
          <dependencies>
            <dependency><groupId>sample</groupId><artifactId>common</artifactId><version>3.0.0</version></dependency>
          </dependencies>
        </project>
        """);
    addArtifact(repository, "sample:shared:1.0.0", "<project/>");
    addArtifact(repository, "sample:shared:2.0.0", "<project/>");
    addArtifact(repository, "sample:common:3.0.0", "<project/>");

    Map<String, String> declarations = new LinkedHashMap<>();
    declarations.put("api", "sample:api:1.0.0");
    declarations.put("shared", "sample:shared:2.0.0");
    MavenResolver resolver = new MavenResolver(temp.resolve("cache"), repository.toUri());

    List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(declarations);

    assertEquals(
        List.of(
            "sample:api:1.0.0", "sample:shared:2.0.0", "sample:util:1.2.0", "sample:common:3.0.0"),
        resolved.stream().map(result -> result.coordinate().notation()).toList());
    assertTrue(resolved.stream().allMatch(result -> Files.isRegularFile(result.jar())));
  }

  @Test
  void rendersDependencyTreeWithDepthScopesAndOptionalMarkers() throws Exception {
    Path repository = temp.resolve("tree-repository");
    addArtifact(
        repository,
        "sample:root:1.0",
        """
        <project><groupId>sample</groupId><artifactId>root</artifactId><version>1.0</version>
          <dependencies>
            <dependency><groupId>sample</groupId><artifactId>library</artifactId><version>1.0</version>
              <exclusions><exclusion><groupId>sample</groupId><artifactId>blocked</artifactId></exclusion></exclusions>
            </dependency>
            <dependency><groupId>sample</groupId><artifactId>child</artifactId><version>1.0</version></dependency>
            <dependency><groupId>sample</groupId><artifactId>provided</artifactId><version>1.0</version><scope>provided</scope></dependency>
            <dependency><groupId>sample</groupId><artifactId>optional</artifactId><version>1.0</version><optional>true</optional></dependency>
          </dependencies>
        </project>
        """);
    addArtifact(
        repository,
        "sample:library:1.0",
        """
        <project><groupId>sample</groupId><artifactId>library</artifactId><version>1.0</version>
          <dependencies>
            <dependency><groupId>sample</groupId><artifactId>child</artifactId><version>1.0</version><scope>runtime</scope></dependency>
            <dependency><groupId>sample</groupId><artifactId>blocked</artifactId><version>1.0</version></dependency>
          </dependencies>
        </project>
        """);
    addArtifact(repository, "sample:child:1.0", "<project/>");
    addArtifact(repository, "sample:provided:1.0", "<project/>");
    addArtifact(repository, "sample:optional:1.0", "<project/>");
    MavenResolver resolver = new MavenResolver(temp.resolve("tree-cache"), repository.toUri());

    List<MavenResolver.TreeEntry> entries =
        resolver.resolveDependencyTree(MavenCoordinate.parse("sample:root:1.0"), 2);

    assertEquals(
        List.of(
            "sample:root:1.0",
            "sample:library:1.0",
            "sample:child:1.0",
            "sample:blocked:1.0",
            "sample:child:1.0",
            "sample:provided:1.0",
            "sample:optional:1.0"),
        entries.stream().map(entry -> entry.coordinate().notation()).toList());
    assertEquals("excluded", entries.get(3).note());
    assertNull(entries.get(4).note());
    assertEquals("scope omitted", entries.get(5).note());
    assertTrue(entries.get(6).optional());
    assertEquals(
        List.of(
            "sample:root:1.0",
            "sample:library:1.0",
            "sample:child:1.0",
            "sample:provided:1.0",
            "sample:optional:1.0"),
        resolver.resolveDependencyTree(MavenCoordinate.parse("sample:root:1.0"), 1).stream()
            .map(entry -> entry.coordinate().notation())
            .toList());
    assertEquals(
        "sample:root:1.0"
            + System.lineSeparator()
            + "├── sample:library:1.0 [compile]"
            + System.lineSeparator()
            + "│   ├── sample:child:1.0 [runtime]"
            + System.lineSeparator()
            + "│   └── sample:blocked:1.0 [compile] excluded"
            + System.lineSeparator()
            + "├── sample:child:1.0 [compile]"
            + System.lineSeparator()
            + "├── sample:provided:1.0 [provided] scope omitted"
            + System.lineSeparator()
            + "└── sample:optional:1.0 [compile optional]"
            + System.lineSeparator(),
        dev.jot.core.DependencyTree.render(entries));
  }

  @Test
  void resolvesPinnedCoordinatesAndEffectiveDirectPomDependencies() throws Exception {
    Path repository = temp.resolve("direct-dependencies-repository");
    addArtifact(
        repository,
        "sample:api:1.0.0",
        """
        <project><groupId>sample</groupId><artifactId>api</artifactId><version>1.0.0</version>
          <dependencyManagement><dependencies>
            <dependency><groupId>sample</groupId><artifactId>managed</artifactId><version>2.0.0</version></dependency>
          </dependencies></dependencyManagement>
          <dependencies>
            <dependency><groupId>sample</groupId><artifactId>managed</artifactId></dependency>
            <dependency><groupId>sample</groupId><artifactId>runtime</artifactId><version>3.0.0</version><scope>runtime</scope><optional>true</optional></dependency>
          </dependencies>
        </project>
        """);
    MavenResolver resolver =
        new MavenResolver(temp.resolve("direct-dependencies-cache"), repository.toUri());

    MavenCoordinate coordinate = resolver.resolveCoordinate("sample:api:1.0.0");
    List<MavenResolver.DirectDependency> dependencies =
        resolver.resolveDirectDependencies(coordinate);

    assertEquals("sample:api:1.0.0", coordinate.notation());
    assertEquals(
        List.of("sample:managed:2.0.0 [compile]", "sample:runtime:3.0.0 [runtime optional]"),
        dependencies.stream()
            .map(
                dependency ->
                    dependency.coordinate().notation()
                        + " ["
                        + dependency.scope()
                        + (dependency.optional() ? " optional" : "")
                        + "]")
            .toList());
  }

  @Test
  void readsLatestReleaseMetadataAndUsesTheCacheOffline() throws Exception {
    Path repository = temp.resolve("metadata-repository");
    Path metadata = repository.resolve("sample/library/maven-metadata.xml");
    Files.createDirectories(metadata.getParent());
    Files.writeString(
        metadata,
        """
        <metadata><groupId>sample</groupId><artifactId>library</artifactId>
          <versioning><latest>1.3.0-SNAPSHOT</latest><release>1.2.0</release>
            <versions><version>1.0.0</version><version>1.2.0</version></versions>
          </versioning>
        </metadata>
        """);
    Path cache = temp.resolve("metadata-cache");
    MavenCoordinate coordinate = MavenCoordinate.parse("sample:library:1.0.0");

    assertEquals(
        java.util.Optional.of("1.2.0"),
        new MavenResolver(cache, repository.toUri()).latestAvailableVersion(coordinate));
    assertEquals(
        java.util.Optional.of("1.2.0"),
        new MavenResolver(cache, repository.toUri(), true).latestAvailableVersion(coordinate));
  }

  @Test
  void rejectsAnArtifactWhenTheRepositoryChecksumDoesNotMatch() throws Exception {
    Path repository = temp.resolve("bad-checksum-repository");
    String coordinateText = "sample:tampered:1.0.0";
    MavenCoordinate coordinate = MavenCoordinate.parse(coordinateText);
    addArtifact(repository, coordinateText, "<project/>");
    Path checksum = repository.resolve(coordinate.repositoryPath("jar") + ".sha256");
    Files.writeString(
        checksum, "0000000000000000000000000000000000000000000000000000000000000000\n");
    MavenResolver resolver = new MavenResolver(temp.resolve("bad-cache"), repository.toUri());

    IOException error =
        assertThrows(IOException.class, () -> resolver.resolve(Map.of("tampered", coordinateText)));

    assertTrue(error.getMessage().contains("SHA-256 mismatch"));
  }

  @Test
  void writesVerifiesAndUsesLockfileArtifactsOffline() throws Exception {
    Path repository = temp.resolve("locked-repository");
    Path cache = temp.resolve("locked-cache");
    String notation = "sample:locked:1.0.0";
    addArtifact(repository, notation, "<project/>");
    MavenResolver online = new MavenResolver(cache, repository.toUri());
    List<MavenResolver.ResolvedArtifact> resolved = online.resolve(Map.of("locked", notation));
    Path lockPath = temp.resolve("jot.lock");
    Lockfile.create(List.of(notation), resolved).write(lockPath);

    Lockfile lock = Lockfile.read(lockPath);
    lock.verify(List.of(notation), resolved);
    assertEquals("sample:locked:1.0.0", lock.roots().getFirst().notation());
    assertTrue(Files.readString(lockPath).contains("sha256 = \""));
    IOException stale =
        assertThrows(IOException.class, () -> lock.verify(List.of("sample:other:1.0.0"), resolved));
    assertTrue(stale.getMessage().contains("out of date"));

    MavenResolver offline = new MavenResolver(cache, repository.toUri(), true);
    List<MavenResolver.ResolvedArtifact> cached = offline.resolve(Map.of("locked", notation));
    lock.verify(List.of(notation), cached);

    IOException missing =
        assertThrows(
            IOException.class, () -> offline.resolve(Map.of("missing", "sample:missing:1.0.0")));
    assertTrue(missing.getMessage().contains("offline mode"));
    Files.writeString(cached.getFirst().jar(), "tampered");
    IOException mismatch =
        assertThrows(IOException.class, () -> lock.verify(List.of(notation), cached));
    assertTrue(mismatch.getMessage().contains("checksum mismatch"));
  }

  private void addArtifact(Path repository, String notation, String pomContents) throws Exception {
    MavenCoordinate coordinate = MavenCoordinate.parse(notation);
    Path pom = repository.resolve(coordinate.pomPath());
    Files.createDirectories(pom.getParent());
    Files.writeString(pom, pomContents);
    Path jar = repository.resolve(coordinate.repositoryPath("jar"));
    Files.createDirectories(jar.getParent());
    try (OutputStream output = Files.newOutputStream(jar);
        JarOutputStream archive = new JarOutputStream(output)) {
      // Empty but valid Maven artifact for resolver graph tests.
    }
  }
}
