package dev.jot.core;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.List;
import java.util.Locale;
import javax.lang.model.SourceVersion;

/** Creates small Jot-native Java applications and libraries. */
public final class ProjectInitializer {
  private static final String JUNIT = "org.junit.jupiter:junit-jupiter:5.11.0";
  private static final String JUNIT_CONSOLE = "org.junit.platform:junit-platform-console:1.11.0";

  private ProjectInitializer() {}

  public static Path initialize(Path destination, String name, String template, String packageName)
      throws IOException {
    return initialize(destination, name, template, null, packageName);
  }

  public static Path initialize(
      Path destination, String name, String template, String group, String packageName)
      throws IOException {
    Path root = destination.toAbsolutePath().normalize();
    String selectedTemplate = normalizeTemplate(template);
    validateProjectName(name);
    String javaGroup = group == null ? "com.example" : group;
    validatePackage(javaGroup);
    String javaPackage = packageName == null ? javaGroup : packageName;
    validatePackage(javaPackage);

    List<FileContent> files;
    if (selectedTemplate.equals("java-workspace")) {
      files = workspaceFiles(name, javaGroup, javaPackage);
    } else {
      String packagePath = javaPackage.replace('.', '/');
      String className =
          switch (selectedTemplate) {
            case "java-lib" -> "Greeting";
            case "java-server" -> "ServerMain";
            default -> "Main";
          };
      String projectToml =
          projectManifest(name, selectedTemplate, javaGroup, javaPackage, className);
      String source = sourceFile(selectedTemplate, javaPackage, className);
      String test = testFile(javaPackage, className, selectedTemplate);
      files =
          List.of(
              new FileContent(Path.of("jot.toml"), projectToml),
              new FileContent(
                  Path.of("src", "main", "java", packagePath, className + ".java"), source),
              new FileContent(
                  Path.of("src", "test", "java", packagePath, className + "Test.java"), test),
              new FileContent(Path.of("README.md"), readme(name, selectedTemplate)),
              new FileContent(Path.of(".gitignore"), "target/\n.jot.lock\n"));
    }

    for (FileContent file : files) {
      Path output = root.resolve(file.path()).normalize();
      if (!output.startsWith(root)) throw new IllegalArgumentException("invalid generated path");
      if (Files.exists(output)) {
        throw new IOException("refusing to overwrite existing file: " + output);
      }
    }
    Files.createDirectories(root);
    for (FileContent file : files) {
      Path output = root.resolve(file.path());
      Files.createDirectories(output.getParent());
      Files.writeString(
          output, file.contents(), StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE);
    }
    return root;
  }

  private static String normalizeTemplate(String template) {
    return switch (template == null ? "java-minimal" : template) {
      case "app", "minimal", "java-minimal" -> "java-minimal";
      case "library", "lib", "java-lib" -> "java-lib";
      case "cli", "java-cli" -> "java-cli";
      case "server", "java-server" -> "java-server";
      case "workspace", "java-workspace" -> "java-workspace";
      default ->
          throw new IllegalArgumentException(
              "template must be `java-minimal`, `java-lib`, `java-cli`, `java-server`, or"
                  + " `java-workspace`");
    };
  }

  private static String projectManifest(
      String name, String template, String group, String javaPackage, String className) {
    StringBuilder manifest =
        new StringBuilder(
            """
            [project]
            name = "%s"
            version = "0.1.0"
            """
                .formatted(name));
    manifest.append("group = \"").append(group).append("\"\n");
    manifest.append("layout = \"maven\"\n");
    if (!template.equals("java-lib")) {
      manifest
          .append("main-class = \"")
          .append(javaPackage)
          .append('.')
          .append(className)
          .append("\"\n");
    }
    manifest.append(
        """

        [toolchains]
        java = "27"

        [test-dependencies]
        junit = "%s"
        """
            .formatted(JUNIT));
    manifest.append("junit-console = \"").append(JUNIT_CONSOLE).append("\"\n");
    if (template.equals("java-cli")) {
      manifest.append("\n[dependencies]\npicocli = \"info.picocli:picocli:4.7.6\"\n");
    }
    return manifest.toString();
  }

  private static List<FileContent> workspaceFiles(String name, String group, String packageName) {
    String domain = packageName + ".domain";
    String api = packageName + ".api";
    String cli = packageName + ".cli";
    String domainPath = domain.replace('.', '/');
    String apiPath = api.replace('.', '/');
    String cliPath = cli.replace('.', '/');
    String moduleDependencies = "\n[test-dependencies]\njunit = \"" + JUNIT + "\"\n";
    String testRunner = "junit-console = \"" + JUNIT_CONSOLE + "\"\n";
    return List.of(
        new FileContent(
            Path.of("jot.toml"),
            "[workspace]\nmembers = [\"domain\", \"api\", \"cli\"]\ngroup = \""
                + group
                + "\"\n\n[toolchains]\njava = \"27\"\n"),
        new FileContent(Path.of(".gitignore"), "target/\n**/target/\n.jot.lock\n"),
        new FileContent(
            Path.of("README.md"),
            "# "
                + name
                + "\n\n"
                + "Java workspace generated by Jot.\n\n"
                + "Modules: `domain`, `api`, `cli`.\n\n"
                + "```sh\n"
                + "jot build\n"
                + "jot test\n"
                + "jot run --module cli -- Java\n"
                + "jot run --module api\n"
                + "```\n"),
        new FileContent(
            Path.of("domain", "jot.toml"),
            moduleManifest(name + "-domain", group, null) + moduleDependencies + testRunner),
        new FileContent(
            Path.of("domain", "src", "main", "java", domainPath, "Order.java"),
            "package " + domain + ";\n\npublic record Order(String id, String customer) {}\n"),
        new FileContent(
            Path.of("domain", "src", "test", "java", domainPath, "OrderTest.java"),
            "package "
                + domain
                + ";\n\n"
                + "import static org.junit.jupiter.api.Assertions.assertEquals;\n"
                + "import org.junit.jupiter.api.Test;\n\n"
                + "class OrderTest {\n"
                + "    @Test void keepsItsId() { assertEquals(\"A-1\", new Order(\"A-1\","
                + " \"Jot\").id()); }\n"
                + "}\n"),
        new FileContent(
            Path.of("api", "jot.toml"),
            moduleManifest(name + "-api", group, api + ".ServerMain")
                + "\n[dependencies]\ndomain = { path = \"../domain\" }\n"
                + moduleDependencies
                + testRunner),
        new FileContent(
            Path.of("api", "src", "main", "java", apiPath, "ServerMain.java"),
            serverSource(api, domain)),
        new FileContent(
            Path.of("cli", "jot.toml"),
            moduleManifest(name + "-cli", group, cli + ".Main")
                + "\n[dependencies]\ndomain = { path = \"../domain\" }\n"
                + moduleDependencies
                + testRunner),
        new FileContent(
            Path.of("cli", "src", "main", "java", cliPath, "Main.java"),
            "package "
                + cli
                + ";\n\nimport "
                + domain
                + ".Order;\n\n"
                + "public final class Main {\n"
                + "    private Main() {}\n"
                + "    public static String message(String customer) {\n"
                + "        return \"generated order for \" + new Order(\"A-1\","
                + " customer).customer();\n"
                + "    }\n"
                + "    public static void main(String[] args) {\n"
                + "        System.out.println(message(args.length == 0 ? \"Jot\" : args[0]));\n"
                + "    }\n"
                + "}\n"),
        new FileContent(
            Path.of("cli", "src", "test", "java", cliPath, "MainTest.java"),
            "package "
                + cli
                + ";\n\n"
                + "import static org.junit.jupiter.api.Assertions.assertEquals;\n"
                + "import org.junit.jupiter.api.Test;\n\n"
                + "class MainTest {\n"
                + "    @Test void createsMessage() { assertEquals(\"generated order for Jot\","
                + " Main.message(\"Jot\")); }\n"
                + "}\n"));
  }

  private static String moduleManifest(String name, String group, String mainClass) {
    String manifest =
        "[project]\nname = \""
            + name
            + "\"\nversion = \"0.1.0\"\ngroup = \""
            + group
            + "\"\nlayout = \"maven\"\n";
    return mainClass == null ? manifest : manifest + "main-class = \"" + mainClass + "\"\n";
  }

  private static String serverSource(String apiPackage, String domainPackage) {
    return "package "
        + apiPackage
        + ";\n\nimport "
        + domainPackage
        + ".Order;\n"
        + "import com.sun.net.httpserver.HttpServer;\n"
        + "import java.io.IOException;\n"
        + "import java.io.OutputStream;\n"
        + "import java.net.InetSocketAddress;\n"
        + "import java.nio.charset.StandardCharsets;\n\n"
        + "public final class ServerMain {\n"
        + "    private ServerMain() {}\n"
        + "    public static HttpServer createServer(int port) throws IOException {\n"
        + "        HttpServer server = HttpServer.create(new InetSocketAddress(port), 0);\n"
        + "        server.createContext(\"/health\", exchange -> {\n"
        + "            byte[] body = (\"ok:\" + new Order(\"A-1\","
        + " \"Jot\").id()).getBytes(StandardCharsets.UTF_8);\n"
        + "            exchange.sendResponseHeaders(200, body.length);\n"
        + "            try (OutputStream output = exchange.getResponseBody()) { output.write(body);"
        + " }\n"
        + "        });\n"
        + "        return server;\n"
        + "    }\n"
        + "    public static void main(String[] args) throws IOException {\n"
        + "        createServer(8080).start();\n"
        + "        System.out.println(\"API listening on :8080\");\n"
        + "    }\n"
        + "}\n";
  }

  private static String sourceFile(String template, String javaPackage, String className) {
    if (template.equals("java-server")) {
      return """
      package %s;

      import com.sun.net.httpserver.HttpServer;
      import java.io.IOException;
      import java.io.OutputStream;
      import java.net.InetSocketAddress;
      import java.nio.charset.StandardCharsets;

      public final class ServerMain {
          private ServerMain() {}

          public static HttpServer createServer(int port) throws IOException {
              HttpServer server = HttpServer.create(new InetSocketAddress(port), 0);
              server.createContext("/health", exchange -> {
                  byte[] body = "ok".getBytes(StandardCharsets.UTF_8);
                  exchange.sendResponseHeaders(200, body.length);
                  try (OutputStream output = exchange.getResponseBody()) {
                      output.write(body);
                  }
              });
              return server;
          }

          public static void main(String[] args) throws IOException {
              createServer(8080).start();
              System.out.println("server listening on http://localhost:8080/health");
          }
      }
      """
          .formatted(javaPackage);
    }
    if (template.equals("java-cli")) {
      return """
      package %s;

      import picocli.CommandLine;
      import picocli.CommandLine.Command;
      import picocli.CommandLine.Option;

      @Command(name = "%s", mixinStandardHelpOptions = true, version = "1.0")
      public final class Main implements Runnable {
          @Option(names = {"-n", "--name"}, description = "Name to greet", defaultValue = "world")
          private String name;

          public static String greeting(String value) {
              return "Hello, " + value + "!";
          }

          @Override
          public void run() {
              System.out.println(greeting(name));
          }

          public static void main(String[] args) {
              System.exit(new CommandLine(new Main()).execute(args));
          }
      }
      """
          .formatted(javaPackage, className.toLowerCase(Locale.ROOT));
    }
    if (!template.equals("java-lib")) {
      return """
      package %s;

      public final class Main {
          private Main() {}

          public static String greeting(String name) {
              return "Hello, " + name + "!";
          }

          public static void main(String[] args) {
              String name = args.length == 0 ? "Java" : args[0];
              System.out.println(greeting(name));
          }
      }
      """
          .formatted(javaPackage);
    }
    return """
    package %s;

    public final class Greeting {
        private Greeting() {}

        public static String message(String name) {
            return "Hello, " + name + "!";
        }
    }
    """
        .formatted(javaPackage);
  }

  private static String testFile(String javaPackage, String className, String template) {
    if (template.equals("java-server")) {
      return """
      package %s;

      import static org.junit.jupiter.api.Assertions.assertEquals;

      import com.sun.net.httpserver.HttpServer;
      import java.net.URI;
      import java.net.http.HttpClient;
      import java.net.http.HttpRequest;
      import java.net.http.HttpResponse;
      import org.junit.jupiter.api.Test;

      class ServerMainTest {
          @Test
          void servesHealthRequests() throws Exception {
              HttpServer server = ServerMain.createServer(0);
              try {
                  server.start();
                  URI uri = URI.create("http://localhost:" + server.getAddress().getPort() + "/health");
                  HttpResponse<String> response = HttpClient.newHttpClient().send(
                      HttpRequest.newBuilder(uri).GET().build(), HttpResponse.BodyHandlers.ofString());
                  assertEquals(200, response.statusCode());
                  assertEquals("ok", response.body());
              } finally {
                  server.stop(0);
              }
          }
      }
      """
          .formatted(javaPackage);
    }
    String method = template.equals("java-lib") ? "message" : "greeting";
    return """
    package %s;

    import org.junit.jupiter.api.Test;

    import static org.junit.jupiter.api.Assertions.assertEquals;

    class %sTest {
        @Test
        void createsAGreeting() {
            assertEquals("Hello, Java!", %s.%s("Java"));
        }
    }
    """
        .formatted(javaPackage, className, className, method);
  }

  private static String readme(String name, String template) {
    return """
    # %s

    Jot %s project.

    ```sh
    jot build
    jot test
    %s
    ```
    """
        .formatted(
            name,
            template,
            template.equals("java-lib")
                ? ""
                : template.equals("java-server")
                    ? "jot run"
                    : template.equals("java-workspace")
                        ? "jot run --module cli -- Java"
                        : "jot run -- Java");
  }

  private static void validateProjectName(String name) {
    if (name == null || !name.matches("[A-Za-z][A-Za-z0-9_-]*")) {
      throw new IllegalArgumentException(
          "project name must start with a letter and contain only letters, numbers, `-`, or `_`");
    }
  }

  private static void validatePackage(String packageName) {
    if (packageName == null || packageName.isBlank()) {
      throw new IllegalArgumentException("Java package name cannot be blank");
    }
    for (String part : packageName.split("\\.", -1)) {
      if (!SourceVersion.isIdentifier(part) || SourceVersion.isKeyword(part)) {
        throw new IllegalArgumentException("invalid Java package name: " + packageName);
      }
    }
  }

  private record FileContent(Path path, String contents) {}
}
