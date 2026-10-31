package dev.jot.cli;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/** Turns command-line tokens into the immutable command model. */
final class CommandLineParser {
  private CommandLineParser() {}

  static CommandLine parse(String[] arguments) {
    Arguments args = new Arguments(arguments);
    boolean offline = false;
    while (args.consumeLeading("--offline")) offline = true;
    if (!args.hasNext() || isHelp(args.peek())) return special(true, false);
    if (isVersion(args.peek())) return special(false, true);

    Options options = new Options(args.next(), offline);
    while (args.hasNext()) {
      String token = args.next();
      if (token.equals("--") && options.command.equals("run")) {
        options.runArguments.addAll(args.remaining());
        break;
      }
      if (token.startsWith("--")) options.parseOption(token, args);
      else options.parsePositional(token);
    }
    options.validate();
    return options.toCommandLine();
  }

  private static boolean isHelp(String token) {
    return token.equals("--help") || token.equals("-h");
  }

  private static boolean isVersion(String token) {
    return token.equals("--version") || token.equals("-V");
  }

  private static CommandLine special(boolean help, boolean version) {
    return new CommandLine(help, version, false, null, null);
  }

  private static final class Arguments {
    private final String[] values;
    private int index;

    private Arguments(String[] values) {
      this.values = values;
    }

    private boolean hasNext() {
      return index < values.length;
    }

    private String peek() {
      return values[index];
    }

    private String next() {
      return values[index++];
    }

    private boolean consumeLeading(String token) {
      if (!hasNext() || !peek().equals(token)) return false;
      next();
      return true;
    }

    private String valueFor(String option) {
      if (!hasNext() || peek().equals("--")) {
        throw new IllegalArgumentException(option + " needs a value");
      }
      return next();
    }

    private List<String> remaining() {
      List<String> result = List.of(values).subList(index, values.length);
      index = values.length;
      return result;
    }
  }

  private static final class Options {
    private final String command;
    private final List<String> runArguments = new ArrayList<>();
    private Path projectRoot;
    private String module;
    private String projectName;
    private String template;
    private String packageName;
    private String dependencyCoordinate;
    private String dependencyName;
    private String dependencyCatalog;
    private String toolchainAction;
    private String toolchainSpec;
    private boolean toolchainForce;
    private boolean offline;
    private boolean testDependency;
    private int treeDepth = 3;
    private boolean treeWorkspace;
    private boolean workspacePin;
    private boolean resolveDependencies;
    private boolean formatCheck;

    private Options(String command, boolean offline) {
      this.command = command;
      this.offline = offline;
    }

    private void parseOption(String option, Arguments args) {
      switch (option) {
        case "--project" ->
            projectRoot = Path.of(args.valueFor(option)).toAbsolutePath().normalize();
        case "--module" -> module = args.valueFor(option);
        case "--depth" -> parseDepth(args.valueFor(option));
        case "--check" -> {
          require(command.equals("fmt"), option);
          formatCheck = true;
        }
        case "--deps" -> {
          require(command.equals("resolve"), option);
          resolveDependencies = true;
        }
        case "--workspace" -> {
          require(command.equals("tree") || command.equals("toolchain"), option);
          if (command.equals("tree")) treeWorkspace = true;
          else workspacePin = true;
        }
        case "--offline" -> offline = true;
        case "--force" -> {
          require(command.equals("toolchain"), option);
          toolchainForce = true;
        }
        case "--test" -> {
          require(command.equals("add") || command.equals("remove"), option);
          testDependency = true;
        }
        case "--name" -> {
          require(command.equals("add"), option);
          dependencyName = args.valueFor(option);
        }
        case "--catalog" -> {
          require(command.equals("add"), option);
          dependencyCatalog = args.valueFor(option);
        }
        case "--template" -> {
          require(command.equals("init"), option);
          template = args.valueFor(option);
        }
        case "--package" -> {
          require(command.equals("init"), option);
          packageName = args.valueFor(option);
        }
        default -> throw new IllegalArgumentException("unknown argument: " + option);
      }
    }

    private void parseDepth(String value) {
      require(command.equals("tree"), "--depth");
      try {
        treeDepth = Integer.parseInt(value);
      } catch (NumberFormatException error) {
        throw new IllegalArgumentException("--depth needs a non-negative integer");
      }
      if (treeDepth < 0) throw new IllegalArgumentException("--depth needs a non-negative integer");
    }

    private void parsePositional(String value) {
      switch (command) {
        case "init" -> projectName = once(projectName, value, "init accepts one project name");
        case "add" ->
            dependencyCoordinate =
                once(dependencyCoordinate, value, "add accepts one dependency coordinate");
        case "remove" ->
            dependencyName = once(dependencyName, value, "remove accepts one dependency name");
        case "tree", "resolve" ->
            dependencyCoordinate =
                once(dependencyCoordinate, value, command + " accepts one dependency coordinate");
        case "toolchain" -> parseToolchainPositional(value);
        default -> throw new IllegalArgumentException("unknown argument: " + value);
      }
    }

    private static String once(String current, String value, String error) {
      if (current != null) throw new IllegalArgumentException(error);
      return value;
    }

    private void parseToolchainPositional(String value) {
      if (toolchainAction == null) {
        if (!List.of("list", "pin", "install").contains(value)) {
          throw new IllegalArgumentException("unknown toolchain command: " + value);
        }
        toolchainAction = value;
      } else if ((toolchainAction.equals("pin") || toolchainAction.equals("install"))
          && toolchainSpec == null) {
        toolchainSpec = value;
      } else {
        throw new IllegalArgumentException("unexpected toolchain argument: " + value);
      }
    }

    private void validate() {
      if (command.equals("add")) {
        if (dependencyCoordinate == null && dependencyCatalog == null)
          throw new IllegalArgumentException("add needs a coordinate or --catalog alias");
        if (dependencyCoordinate != null && dependencyCatalog != null)
          throw new IllegalArgumentException("use either a dependency coordinate or --catalog");
      }
      if (command.equals("remove") && dependencyName == null)
        throw new IllegalArgumentException("remove needs a dependency name");
      if (command.equals("tree")) {
        if (dependencyCoordinate == null && !treeWorkspace)
          throw new IllegalArgumentException("tree needs a dependency coordinate or --workspace");
        if (treeWorkspace && dependencyCoordinate != null)
          throw new IllegalArgumentException("tree dependency cannot be combined with --workspace");
      }
      if (command.equals("resolve") && dependencyCoordinate == null)
        throw new IllegalArgumentException("resolve needs a dependency coordinate");
      if (!command.equals("toolchain")) return;
      if (toolchainAction == null)
        throw new IllegalArgumentException("toolchain needs list, pin, or install");
      if ((toolchainAction.equals("pin") || toolchainAction.equals("install"))
          && toolchainSpec == null)
        throw new IllegalArgumentException(
            "toolchain " + toolchainAction + " needs java@<version>");
      if (workspacePin && !toolchainAction.equals("pin"))
        throw new IllegalArgumentException("--workspace only applies to toolchain pin");
      if (toolchainForce && !toolchainAction.equals("install"))
        throw new IllegalArgumentException("--force only applies to toolchain install");
    }

    private CommandLine toCommandLine() {
      CommandLine.Command parsed =
          switch (command) {
            case "build" -> new CommandLine.Build(module);
            case "run" -> new CommandLine.Run(module, List.copyOf(runArguments));
            case "test" -> new CommandLine.Test(module);
            case "doc" -> new CommandLine.Doc(module);
            case "lint" -> new CommandLine.Lint(module);
            case "fmt" -> new CommandLine.Fmt(module, formatCheck);
            case "deps" -> new CommandLine.Deps(module);
            case "outdated" -> new CommandLine.Outdated(module);
            case "add" ->
                new CommandLine.Add(
                    dependencyCoordinate, dependencyName, dependencyCatalog, testDependency);
            case "remove" -> new CommandLine.Remove(dependencyName, testDependency);
            case "lock" -> new CommandLine.Lock();
            case "clean" -> new CommandLine.Clean();
            case "init" -> new CommandLine.Init(projectName, template, packageName);
            case "tree" ->
                new CommandLine.Tree(module, dependencyCoordinate, treeDepth, treeWorkspace);
            case "resolve" -> new CommandLine.Resolve(dependencyCoordinate, resolveDependencies);
            case "toolchain" -> new CommandLine.Toolchain(toolchainAction());
            default -> throw new IllegalArgumentException("unknown command: " + command);
          };
      return new CommandLine(false, false, offline, projectRoot, parsed);
    }

    private CommandLine.ToolchainAction toolchainAction() {
      return switch (toolchainAction) {
        case "list" -> new CommandLine.ListToolchains();
        case "pin" -> new CommandLine.PinToolchain(toolchainSpec, workspacePin);
        case "install" -> new CommandLine.InstallToolchain(toolchainSpec, toolchainForce);
        default ->
            throw new IllegalArgumentException("unknown toolchain command: " + toolchainAction);
      };
    }

    private static void require(boolean condition, String option) {
      if (!condition) throw new IllegalArgumentException("unknown argument: " + option);
    }
  }
}
