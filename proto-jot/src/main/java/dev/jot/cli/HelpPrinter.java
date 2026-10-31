package dev.jot.cli;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Renders root and subcommand help from the CLI command descriptions and option metadata. */
final class HelpPrinter {
  private static final List<HelpSpec> COMMANDS =
      List.of(
          command(
              "init",
              "Create a new Java project",
              "[name] [OPTIONS]",
              opt("--project <directory>", "Create the project at this directory"),
              opt("--template <name>", "Choose a project template"),
              opt("--group <id>", "Set the project group"),
              opt("--package <name>", "Set the Java package")),
          command(
              "build",
              "Compile and package the current project",
              "[OPTIONS]",
              projectOptions(opt("--module <name>", "Build one workspace module"))),
          command(
              "run",
              "Build and run the project",
              "[OPTIONS] [-- <args>...]",
              projectOptions(opt("--module <name>", "Run one workspace module"))),
          command(
              "test",
              "Build and run project tests",
              "[OPTIONS]",
              projectOptions(opt("--module <name>", "Test one workspace module"))),
          command(
              "bench",
              "Run configured JMH benchmarks",
              "[filter] [OPTIONS]",
              projectOptions(
                  opt("--module <name>", "Select a workspace module"),
                  opt("--forks <count>", "Set the benchmark fork count"),
                  opt("--warmup <count>", "Set the warmup iteration count"),
                  opt("--iterations <count>", "Set the measurement iteration count"))),
          command(
              "audit",
              "Find vulnerable dependencies",
              "[OPTIONS]",
              opt("--fix", "Update dependencies with available fixes"),
              opt("--ci", "Use CI-friendly output"),
              opt("--offline", "Use cached dependencies only")),
          command(
              "doc",
              "Generate Java API documentation",
              "[OPTIONS]",
              projectOptions(
                  opt("--module <name>", "Document one workspace module"),
                  opt("--open", "Open the generated documentation"))),
          command(
              "lint",
              "Compile with Java lint warnings",
              "[OPTIONS]",
              projectOptions(opt("--module <name>", "Lint one workspace module"))),
          command(
              "fmt",
              "Format Java source files",
              "[OPTIONS]",
              projectOptions(
                  opt("--module <name>", "Format one workspace module"),
                  opt("--check", "Check formatting without writing changes"))),
          command(
              "add",
              "Add a Maven dependency",
              "<coordinate> | --catalog <alias> [OPTIONS]",
              projectOptions(
                  opt("--name <alias>", "Choose the dependency name"),
                  opt("--catalog <alias>", "Add a dependency from the version catalog"),
                  opt("--test", "Add as a test dependency"))),
          command(
              "remove",
              "Remove a dependency",
              "<name> [OPTIONS]",
              projectOptions(opt("--test", "Remove a test dependency"))),
          command(
              "deps",
              "List project dependencies",
              "[OPTIONS]",
              projectOptions(opt("--module <name>", "List one workspace module"))),
          command(
              "outdated",
              "Show outdated locked dependencies",
              "[OPTIONS]",
              projectOptions(opt("--module <name>", "Check one workspace module"))),
          command(
              "resolve",
              "Resolve a Maven coordinate",
              "<coordinate> [OPTIONS]",
              opt("--deps", "List its direct dependencies"),
              opt("--offline", "Use only cached artifacts")),
          command(
              "tree",
              "Show a dependency tree",
              "<coordinate> | --workspace [OPTIONS]",
              projectOptions(
                  opt("--workspace", "Show workspace module dependencies"),
                  opt("--module <name>", "Select a workspace module"),
                  opt("--depth <levels>", "Limit dependency depth"))),
          command(
              "toolchain",
              "Manage Java toolchains",
              "<COMMAND>",
              List.of(),
              List.of(
                  command(
                      "toolchain list", "List installed Java toolchains", "", List.of(), List.of()),
                  command(
                      "toolchain pin",
                      "Pin a Java toolchain in the manifest",
                      "java@<version|vendor-version> [--workspace]",
                      opt("--workspace", "Pin the workspace toolchain")),
                  command(
                      "toolchain install",
                      "Install a Java toolchain",
                      "java@<version|vendor-version> [OPTIONS]",
                      opt("--force", "Replace an existing installation"),
                      opt("--offline", "Use cached toolchain archives only")))),
          command(
              "lock",
              "Resolve and write the dependency lockfile",
              "[coordinates...] [OPTIONS]",
              projectOptions(
                  opt("--depth <levels>", "Limit dependency depth"),
                  opt("--output <file>", "Write the lockfile to this path"))),
          command(
              "clean",
              "Remove project build outputs",
              "[OPTIONS]",
              projectOptions(opt("--global", "Clear Jot-managed caches"))),
          command(
              "self",
              "Update or uninstall Jot",
              "<COMMAND>",
              List.of(),
              List.of(
                  command(
                      "self update",
                      "Update Jot from a release",
                      "[OPTIONS]",
                      opt("--version <version>", "Install a specific release"),
                      opt("--check", "Check for updates without installing"),
                      opt("--yes", "Skip confirmation")),
                  command(
                      "self uninstall",
                      "Remove the current Jot installation",
                      "[--yes]",
                      opt("--yes", "Skip confirmation")))),
          command(
              "publish",
              "Publish artifacts to a Maven repository",
              "[OPTIONS]",
              projectOptions(
                  opt("--module <name>", "Publish one workspace module"),
                  opt("--repository <url-or-path>", "Select the Maven repository"),
                  opt("--username <name>", "Set the repository username"),
                  opt("--password <password>", "Set the repository password"),
                  opt("--signing-key <path>", "Sign the published artifacts"),
                  opt("--dry-run", "Build without uploading"),
                  opt("--allow-snapshot", "Allow snapshot publication"))));

  private static final Map<String, HelpSpec> BY_TOPIC = index();

  private HelpPrinter() {}

  static boolean hasTopic(String topic) {
    return BY_TOPIC.containsKey(topic);
  }

  static String render(String topic, String version) {
    if (topic == null || topic.isBlank()) return renderRoot(version);
    HelpSpec spec = BY_TOPIC.get(topic);
    if (spec == null) return renderRoot(version);

    StringBuilder output = new StringBuilder();
    output.append("jot ").append(version).append(" — ").append(spec.description()).append('\n');
    output.append("\nUsage: ").append(spec.usage()).append('\n');
    if (!spec.children().isEmpty()) appendCommands(output, spec.children());
    appendOptions(output, spec.options());
    output.append("\nRun 'jot --help' for the full command list.\n");
    return output.toString();
  }

  private static String renderRoot(String version) {
    StringBuilder output = new StringBuilder();
    output.append("jot ").append(version).append(" — a Java build tool\n");
    output.append("\nUsage: jot [OPTIONS] <COMMAND> [ARGS]\n");
    appendCommands(output, COMMANDS);
    appendOptions(
        output,
        List.of(
            opt("-V, --version", "Print version information"),
            opt("--offline", "Use cached dependencies and toolchains only")));
    output.append("\nRun 'jot <command> --help' for command-specific help.\n");
    return output.toString();
  }

  private static void appendCommands(StringBuilder output, List<HelpSpec> commands) {
    output.append("\nCommands:\n");
    int width =
        commands.stream().mapToInt(spec -> displayName(spec.name()).length()).max().orElse(0);
    for (HelpSpec spec : commands) {
      output
          .append("  ")
          .append(String.format("%-" + width + "s", displayName(spec.name())))
          .append("  ")
          .append(spec.description())
          .append('\n');
    }
  }

  private static void appendOptions(StringBuilder output, List<OptionSpec> options) {
    List<OptionSpec> allOptions = new ArrayList<>();
    allOptions.add(opt("-h, --help", "Print help information"));
    allOptions.addAll(options);
    output.append("\nOptions:\n");
    int width = allOptions.stream().mapToInt(option -> option.syntax().length()).max().orElse(0);
    for (OptionSpec option : allOptions) {
      output
          .append("  ")
          .append(String.format("%-" + width + "s", option.syntax()))
          .append("  ")
          .append(option.description())
          .append('\n');
    }
  }

  private static Map<String, HelpSpec> index() {
    Map<String, HelpSpec> result = new LinkedHashMap<>();
    for (HelpSpec command : COMMANDS) {
      result.put(
          command.name(), command.withUsage("jot " + command.name() + " " + command.synopsis()));
      addChildren(result, command.children());
    }
    return Map.copyOf(result);
  }

  private static void addChildren(Map<String, HelpSpec> result, List<HelpSpec> children) {
    for (HelpSpec child : children) {
      result.put(child.name(), child.withUsage("jot " + child.name() + " " + child.synopsis()));
      addChildren(result, child.children());
    }
  }

  private static HelpSpec command(
      String name, String description, String synopsis, OptionSpec... options) {
    return command(name, description, synopsis, List.of(options), List.of());
  }

  private static HelpSpec command(
      String name,
      String description,
      String synopsis,
      List<OptionSpec> options,
      List<HelpSpec> children) {
    return new HelpSpec(name, description, synopsis, null, options, children);
  }

  private static OptionSpec opt(String syntax, String description) {
    return new OptionSpec(syntax, description);
  }

  private static OptionSpec[] projectOptions(OptionSpec... additional) {
    List<OptionSpec> options = new ArrayList<>();
    options.add(opt("--project <directory>", "Select a project directory"));
    options.add(opt("--offline", "Use cached dependencies and toolchains only"));
    options.addAll(List.of(additional));
    return options.toArray(OptionSpec[]::new);
  }

  private static String displayName(String name) {
    int separator = name.lastIndexOf(' ');
    return separator < 0 ? name : name.substring(separator + 1);
  }

  private record OptionSpec(String syntax, String description) {}

  private record HelpSpec(
      String name,
      String description,
      String synopsis,
      String usage,
      List<OptionSpec> options,
      List<HelpSpec> children) {
    HelpSpec {
      options = List.copyOf(options);
      children = List.copyOf(children);
    }

    HelpSpec withUsage(String value) {
      return new HelpSpec(name, description, synopsis, value, options, children);
    }
  }
}
