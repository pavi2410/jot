package dev.jot.cli;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/** Parsed command and options accepted by the Jot CLI. */
record CommandLine(
    String command,
    Path projectRoot,
    String module,
    List<String> runArguments,
    boolean help,
    boolean version,
    boolean offline,
    String projectName,
    String template,
    String packageName,
    String dependencyCoordinate,
    String dependencyName,
    String dependencyCatalog,
    boolean resolveDependencies,
    boolean testDependency,
    int treeDepth,
    boolean treeWorkspace,
    boolean formatCheck) {

  static CommandLine parse(String[] args) {
    int index = 0;
    boolean offline = false;
    while (index < args.length && args[index].equals("--offline")) {
      offline = true;
      index++;
    }
    if (index == args.length || isHelp(args[index])) return specialCommand(true, false);
    if (isVersion(args[index])) return specialCommand(false, true);

    Options options = new Options(args[index++], offline);
    while (index < args.length) {
      String argument = args[index++];
      if (argument.equals("--") && options.command.equals("run")) {
        options.runArguments.addAll(List.of(args).subList(index, args.length));
        break;
      }
      if (argument.startsWith("--")) {
        index = options.parseOption(argument, args, index);
      } else {
        options.parsePositional(argument);
      }
    }
    options.validate();
    return options.result();
  }

  private static boolean isHelp(String argument) {
    return argument.equals("--help") || argument.equals("-h");
  }

  private static boolean isVersion(String argument) {
    return argument.equals("--version") || argument.equals("-V");
  }

  private static CommandLine specialCommand(boolean help, boolean version) {
    return new CommandLine(
        "", null, null, List.of(), help, version, false, null, null, null, null, null, null, false,
        false, 0, false, false);
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
    private boolean offline;
    private boolean testDependency;
    private int treeDepth = 3;
    private boolean treeWorkspace;
    private boolean resolveDependencies;
    private boolean formatCheck;

    private Options(String command, boolean offline) {
      this.command = command;
      this.offline = offline;
    }

    private int parseOption(String option, String[] args, int index) {
      switch (option) {
        case "--project" ->
            projectRoot = Path.of(value(option, args, index++)).toAbsolutePath().normalize();
        case "--module" -> module = value(option, args, index++);
        case "--depth" -> {
          require(command.equals("tree"), option);
          try {
            treeDepth = Integer.parseInt(value(option, args, index++));
          } catch (NumberFormatException error) {
            throw new IllegalArgumentException("--depth needs a non-negative integer");
          }
          if (treeDepth < 0)
            throw new IllegalArgumentException("--depth needs a non-negative integer");
        }
        case "--check" -> {
          require(command.equals("fmt"), option);
          formatCheck = true;
        }
        case "--deps" -> {
          require(command.equals("resolve"), option);
          resolveDependencies = true;
        }
        case "--workspace" -> {
          require(command.equals("tree"), option);
          treeWorkspace = true;
        }
        case "--offline" -> offline = true;
        case "--test" -> {
          require(command.equals("add") || command.equals("remove"), option);
          testDependency = true;
        }
        case "--name" -> {
          require(command.equals("add"), option);
          dependencyName = value(option, args, index++);
        }
        case "--catalog" -> {
          require(command.equals("add"), option);
          dependencyCatalog = value(option, args, index++);
        }
        case "--template" -> {
          require(command.equals("init"), option);
          template = value(option, args, index++);
        }
        case "--package" -> {
          require(command.equals("init"), option);
          packageName = value(option, args, index++);
        }
        default -> throw new IllegalArgumentException("unknown argument: " + option);
      }
      return index;
    }

    private void parsePositional(String value) {
      switch (command) {
        case "init" -> {
          if (projectName != null)
            throw new IllegalArgumentException("init accepts one project name");
          projectName = value;
        }
        case "add" -> {
          if (dependencyCoordinate != null)
            throw new IllegalArgumentException("add accepts one dependency coordinate");
          dependencyCoordinate = value;
        }
        case "remove" -> {
          if (dependencyName != null)
            throw new IllegalArgumentException("remove accepts one dependency name");
          dependencyName = value;
        }
        case "tree" -> {
          if (dependencyCoordinate != null)
            throw new IllegalArgumentException("tree accepts one dependency coordinate");
          dependencyCoordinate = value;
        }
        case "resolve" -> {
          if (dependencyCoordinate != null)
            throw new IllegalArgumentException("resolve accepts one dependency coordinate");
          dependencyCoordinate = value;
        }
        default -> throw new IllegalArgumentException("unknown argument: " + value);
      }
    }

    private void validate() {
      if (command.equals("add") && dependencyCoordinate == null && dependencyCatalog == null) {
        throw new IllegalArgumentException("add needs a coordinate or --catalog alias");
      }
      if (command.equals("add") && dependencyCoordinate != null && dependencyCatalog != null) {
        throw new IllegalArgumentException("use either a dependency coordinate or --catalog");
      }
      if (command.equals("remove") && dependencyName == null) {
        throw new IllegalArgumentException("remove needs a dependency name");
      }
      if (command.equals("tree") && dependencyCoordinate == null) {
        if (!treeWorkspace)
          throw new IllegalArgumentException("tree needs a dependency coordinate or --workspace");
      }
      if (command.equals("tree") && treeWorkspace && dependencyCoordinate != null) {
        throw new IllegalArgumentException("tree dependency cannot be combined with --workspace");
      }
      if (command.equals("resolve") && dependencyCoordinate == null) {
        throw new IllegalArgumentException("resolve needs a dependency coordinate");
      }
    }

    private CommandLine result() {
      return new CommandLine(
          command,
          projectRoot,
          module,
          List.copyOf(runArguments),
          false,
          false,
          offline,
          projectName,
          template,
          packageName,
          dependencyCoordinate,
          dependencyName,
          dependencyCatalog,
          resolveDependencies,
          testDependency,
          treeDepth,
          treeWorkspace,
          formatCheck);
    }

    private static String value(String option, String[] args, int index) {
      if (index >= args.length || args[index].equals("--")) {
        throw new IllegalArgumentException(option + " needs a value");
      }
      return args[index];
    }

    private static void require(boolean condition, String option) {
      if (!condition) throw new IllegalArgumentException("unknown argument: " + option);
    }
  }
}
