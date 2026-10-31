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

    String packagePath = javaPackage.replace('.', '/');
    String className = selectedTemplate.equals("java-lib") ? "Greeting" : "Main";
    String projectToml = projectManifest(name, selectedTemplate, javaGroup, javaPackage);
    String source = sourceFile(selectedTemplate, javaPackage, className);
    String test = testFile(javaPackage, className, selectedTemplate);
    List<FileContent> files =
        List.of(
            new FileContent(Path.of("jot.toml"), projectToml),
            new FileContent(Path.of("src", packagePath, className + ".java"), source),
            new FileContent(Path.of("test", packagePath, className + "Test.java"), test),
            new FileContent(Path.of("README.md"), readme(name, selectedTemplate)));

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
      default ->
          throw new IllegalArgumentException(
              "template must be `java-minimal`, `java-lib`, or `java-cli`");
    };
  }

  private static String projectManifest(
      String name, String template, String group, String javaPackage) {
    StringBuilder manifest =
        new StringBuilder(
            """
            [project]
            name = "%s"
            version = "0.1.0"
            """
                .formatted(name));
    manifest.append("group = \"").append(group).append("\"\n");
    if (!template.equals("java-lib")) {
      manifest.append("main-class = \"").append(javaPackage).append(".Main\"\n");
    }
    manifest.append(
        """

        [toolchains]
        java = "27"

        [build]
        source-dirs = ["src"]

        [test]
        source-dirs = ["test"]

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

  private static String sourceFile(String template, String javaPackage, String className) {
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
        .formatted(name, template, template.equals("java-lib") ? "" : "jot run -- Java");
  }

  private static String defaultPackage(String name) {
    String normalized = name.replace('-', '_').toLowerCase(Locale.ROOT);
    if (!Character.isJavaIdentifierStart(normalized.charAt(0))) normalized = "p" + normalized;
    return "app." + normalized;
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
