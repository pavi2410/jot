package dev.jot.core.resolve;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.OutputStream;
import java.io.IOException;
import java.net.URI;
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
        addArtifact(repository, "sample:api:1.0.0", """
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
        addArtifact(repository, "sample:util:1.2.0", """
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

        assertEquals(List.of("sample:api:1.0.0", "sample:shared:2.0.0",
                        "sample:util:1.2.0", "sample:common:3.0.0"),
                resolved.stream().map(result -> result.coordinate().notation()).toList());
        assertTrue(resolved.stream().allMatch(result -> Files.isRegularFile(result.jar())));
    }

    @Test
    void rejectsAnArtifactWhenTheRepositoryChecksumDoesNotMatch() throws Exception {
        Path repository = temp.resolve("bad-checksum-repository");
        String coordinateText = "sample:tampered:1.0.0";
        MavenCoordinate coordinate = MavenCoordinate.parse(coordinateText);
        addArtifact(repository, coordinateText, "<project/>");
        Path checksum = repository.resolve(coordinate.repositoryPath("jar") + ".sha256");
        Files.writeString(checksum, "0000000000000000000000000000000000000000000000000000000000000000\n");
        MavenResolver resolver = new MavenResolver(temp.resolve("bad-cache"), repository.toUri());

        IOException error = assertThrows(IOException.class,
                () -> resolver.resolve(Map.of("tampered", coordinateText)));

        assertTrue(error.getMessage().contains("SHA-256 mismatch"));
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
