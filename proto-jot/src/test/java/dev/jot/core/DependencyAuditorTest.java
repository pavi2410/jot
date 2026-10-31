package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import dev.jot.core.resolve.MavenResolver;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class DependencyAuditorTest {
  @TempDir Path temp;

  @Test
  void queriesOsvBuildsFindingsAndAppliesDirectDependencyFixes() throws Exception {
    HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/repo/", DependencyAuditorTest::mavenRepository);
    server.createContext(
        "/v1/querybatch",
        exchange ->
            respond(
                exchange,
                200,
                """
                {"results":[{"vulns":[{"id":"CVE-TEST-1"}]}]}
                """));
    server.createContext(
        "/v1/vulns/CVE-TEST-1",
        exchange ->
            respond(
                exchange,
                200,
                """
                {
                  "id":"CVE-TEST-1",
                  "summary":"A test vulnerability",
                  "severity":[{"type":"CVSS_V3","score":"9.8"}],
                  "affected":[{
                    "package":{"ecosystem":"Maven","name":"org.example:library"},
                    "ranges":[{"events":[{"introduced":"0"},{"fixed":"1.1"}]}]
                  }]
                }
                """));
    server.start();
    try {
      Files.writeString(
          temp.resolve("jot.toml"),
          """
          [project]
          name = "audited-project"
          [toolchains]
          java = "27"
          [dependencies]
          library = "org.example:library:1.0"
          """);
      ProjectConfig config = ProjectConfig.read(temp.resolve("jot.toml"), null);
      ProjectWorkspace workspace = ProjectWorkspace.loadProject(temp, config);
      URI base = URI.create("http://127.0.0.1:" + server.getAddress().getPort());
      MavenResolver resolver =
          new MavenResolver(temp.resolve("cache"), base.resolve("/repo/"), false);
      DependencyAuditor auditor =
          new DependencyAuditor(java.net.http.HttpClient.newHttpClient(), base);

      DependencyAuditor.Report report = auditor.audit(temp, config, workspace, resolver, true);

      assertEquals(1, report.packagesScanned());
      assertEquals(1, report.findings().size());
      assertEquals(
          DependencyAuditor.AuditSeverity.CRITICAL, report.findings().getFirst().severity());
      assertTrue(report.findings().getFirst().severity().failsCi());
      assertEquals("1.1", report.findings().getFirst().fixedVersion());
      assertTrue(report.findings().getFirst().chains().getFirst().size() == 1);
      assertEquals(1, report.fixedDependencies());
      assertTrue(Files.readString(temp.resolve("jot.toml")).contains("org.example:library:1.1"));
      assertTrue(Files.isRegularFile(temp.resolve("jot.lock")));
    } finally {
      server.stop(0);
    }
  }

  private static void mavenRepository(HttpExchange exchange) throws java.io.IOException {
    String path = exchange.getRequestURI().getPath();
    if (path.endsWith(".sha256")) {
      respond(exchange, 404, "");
    } else if (path.endsWith(".pom")) {
      String version = path.contains("/1.1/") ? "1.1" : "1.0";
      respond(
          exchange,
          200,
          "<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>"
              + "<artifactId>library</artifactId><version>"
              + version
              + "</version></project>");
    } else {
      respond(exchange, 200, "fake jar bytes");
    }
  }

  private static void respond(HttpExchange exchange, int status, String body)
      throws java.io.IOException {
    byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
    exchange.sendResponseHeaders(status, bytes.length);
    try (var output = exchange.getResponseBody()) {
      output.write(bytes);
    }
  }
}
