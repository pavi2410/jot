package dev.jot.cli;

/** Parses the small option sets used by ordinary project commands. */
final class SimpleCommandParser {
  private SimpleCommandParser() {}

  static CommandLine.Command parse(String command, CommandArguments args) {
    return switch (command) {
      case "build" -> {
        args.allow("module", "clean", "native", "watch");
        args.noPositionals(command);
        yield new CommandLine.Build(
            args.value("module"), args.flag("clean"), args.flag("native"), args.flag("watch"));
      }
      case "run" -> {
        args.allow("module", "watch");
        args.noPositionals(command);
        yield new CommandLine.Run(args.value("module"), args.runArguments(), args.flag("watch"));
      }
      case "test" -> {
        args.allow("module", "watch");
        args.noPositionals(command);
        yield new CommandLine.Test(args.value("module"), args.flag("watch"));
      }
      case "doc" -> {
        args.allow("module", "open");
        args.noPositionals(command);
        yield new CommandLine.Doc(args.value("module"), args.flag("open"));
      }
      case "lint" -> {
        args.allow("module");
        args.noPositionals(command);
        yield new CommandLine.Lint(args.value("module"));
      }
      case "fmt" -> {
        args.allow("module", "check");
        args.noPositionals(command);
        yield new CommandLine.Fmt(args.value("module"), args.flag("check"));
      }
      case "deps" -> {
        args.allow("module");
        args.noPositionals(command);
        yield new CommandLine.Deps(args.value("module"));
      }
      case "outdated" -> {
        args.allow("module");
        args.noPositionals(command);
        yield new CommandLine.Outdated(args.value("module"));
      }
      case "clean" -> {
        args.allow("global");
        args.noPositionals(command);
        yield new CommandLine.Clean(args.flag("global"));
      }
      default -> throw new IllegalArgumentException("unknown command: " + command);
    };
  }
}
