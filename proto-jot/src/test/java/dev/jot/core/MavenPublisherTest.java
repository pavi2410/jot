package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpServer;
import dev.jot.core.resolve.MavenResolver;
import java.net.InetSocketAddress;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.jar.JarFile;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class MavenPublisherTest {
  @TempDir Path temp;

  @Test
  void buildsStagesSignsChecksumsAndPublishesMavenArtifacts() throws Exception {
    write(
        "src/main/java/sample/PublicApi.java",
        """
        package sample;
        /** A published API. */
        public final class PublicApi {
            /** Returns a greeting. */
            public String greeting() { return "hello"; }
        }
        """);
    write(
        "jot.toml",
        """
        [project]
        name = "sample-api"
        version = "1.2.3"
        group = "org.example"
        layout = "maven"
        [toolchains]
        java = "27"
        [publish]
        license = "Apache-2.0"
        description = "A & B library"
        url = "https://example.org/api"
        scm = "https://example.org/api.git"
        [publish.developer]
        name = "Jot Team"
        email = "team@example.org"
        """);
    ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
    WorkspaceProject project = ProjectWorkspace.loadProject(temp, config).find("sample-api");
    MavenResolver resolver =
        new MavenResolver(temp.resolve("cache"), MavenResolver.defaultRepository(), true);
    Path repository = temp.resolve("maven-repository");
    MavenPublisher.ArtifactSigner signer =
        artifacts -> {
          for (Path artifact : artifacts)
            Files.writeString(
                artifact.resolveSibling(artifact.getFileName() + ".asc"), "signature");
        };

    MavenPublisher.Result result =
        MavenPublisher.publish(
            project, resolver, repository.toString(), null, null, false, false, signer);

    assertEquals("org.example:sample-api:1.2.3", result.coordinate().notation());
    Path published = repository.resolve("org/example/sample-api/1.2.3");
    for (String name :
        List.of(
            "sample-api-1.2.3.jar",
            "sample-api-1.2.3-sources.jar",
            "sample-api-1.2.3-javadoc.jar",
            "sample-api-1.2.3.pom")) {
      assertTrue(Files.isRegularFile(published.resolve(name)));
      assertTrue(Files.isRegularFile(published.resolve(name + ".asc")));
      assertTrue(Files.isRegularFile(published.resolve(name + ".sha256")));
    }
    String pom = Files.readString(published.resolve("sample-api-1.2.3.pom"));
    assertTrue(pom.contains("<groupId>org.example</groupId>"));
    assertTrue(pom.contains("A &amp; B library"));
    try (JarFile sources =
        new JarFile(published.resolve("sample-api-1.2.3-sources.jar").toFile())) {
      assertTrue(sources.getEntry("sample/PublicApi.java") != null);
    }
  }

  @Test
  void uploadsArtifactsToHttpRepositoryWithBasicAuthentication() throws Exception {
    write(
        "src/main/java/sample/RemoteApi.java",
        "package sample; /** Remote API. */ public final class RemoteApi {}\n");
    write(
        "jot.toml",
        """
        [project]
        name = "remote-api"
        version = "2.0.0"
        group = "org.example"
        layout = "maven"
        [toolchains]
        java = "27"
        [publish]
        license = "Apache-2.0"
        description = "Remote example"
        url = "https://example.org"
        scm = "https://example.org/repo.git"
        [publish.developer]
        name = "Jot Team"
        """);
    ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
    WorkspaceProject project = ProjectWorkspace.loadProject(temp, config).find("remote-api");
    MavenResolver resolver =
        new MavenResolver(temp.resolve("cache"), MavenResolver.defaultRepository(), true);
    Map<String, byte[]> uploaded = new ConcurrentHashMap<>();
    AtomicBoolean authenticated = new AtomicBoolean(true);
    String expectedAuth =
        "Basic "
            + java.util.Base64.getEncoder()
                .encodeToString(
                    "publisher:secret".getBytes(java.nio.charset.StandardCharsets.UTF_8));
    HttpServer server = HttpServer.create(new InetSocketAddress("localhost", 0), 0);
    server.createContext(
        "/maven",
        exchange -> {
          if (!expectedAuth.equals(exchange.getRequestHeaders().getFirst("Authorization")))
            authenticated.set(false);
          String path = exchange.getRequestURI().getPath().substring("/maven/".length());
          if (exchange.getRequestMethod().equals("HEAD")) {
            int status = uploaded.containsKey(path) ? 200 : 404;
            exchange.sendResponseHeaders(status, -1);
          } else if (exchange.getRequestMethod().equals("PUT")) {
            uploaded.put(path, exchange.getRequestBody().readAllBytes());
            exchange.sendResponseHeaders(201, -1);
          } else {
            exchange.sendResponseHeaders(405, -1);
          }
          exchange.close();
        });
    server.start();
    try {
      MavenPublisher.publish(
          project,
          resolver,
          "http://localhost:" + server.getAddress().getPort() + "/maven",
          "publisher",
          "secret",
          false,
          false,
          artifacts -> {
            for (Path artifact : artifacts)
              Files.writeString(artifact.resolveSibling(artifact.getFileName() + ".asc"), "sig");
          });
    } finally {
      server.stop(0);
    }

    assertTrue(authenticated.get());
    assertTrue(uploaded.containsKey("org/example/remote-api/2.0.0/remote-api-2.0.0.jar"));
    assertTrue(uploaded.containsKey("org/example/remote-api/2.0.0/remote-api-2.0.0.pom"));
    assertTrue(
        uploaded.containsKey("org/example/remote-api/2.0.0/remote-api-2.0.0-sources.jar.sha256"));
  }

  private void write(String relativePath, String contents) throws Exception {
    Path file = temp.resolve(relativePath);
    Files.createDirectories(file.getParent());
    Files.writeString(file, contents);
  }
}
