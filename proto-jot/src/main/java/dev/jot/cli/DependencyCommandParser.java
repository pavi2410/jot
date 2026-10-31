package dev.jot.cli;

/** Parses dependency declaration commands. */
final class DependencyCommandParser {
  private DependencyCommandParser() {}

  static CommandLine.Command parse(String command, CommandArguments args) {
    return switch (command) {
      case "add" -> parseAdd(args);
      case "remove" -> parseRemove(args);
      default -> throw new IllegalArgumentException("unknown command: " + command);
    };
  }

  static CommandLine.Add parseAdd(CommandArguments args) {
    args.allow("name", "catalog", "test");
    if (args.positionals().size() > 1) {
      throw new IllegalArgumentException("add accepts one dependency coordinate");
    }
    String coordinate = args.positionals().isEmpty() ? null : args.positionals().getFirst();
    String catalog = args.value("catalog");
    if (coordinate == null && catalog == null) {
      throw new IllegalArgumentException("add needs a coordinate or --catalog alias");
    }
    if (coordinate != null && catalog != null) {
      throw new IllegalArgumentException("use either a dependency coordinate or --catalog");
    }
    return new CommandLine.Add(coordinate, args.value("name"), catalog, args.flag("test"));
  }

  static CommandLine.Remove parseRemove(CommandArguments args) {
    args.allow("test");
    if (args.positionals().isEmpty()) {
      throw new IllegalArgumentException("remove needs a dependency name");
    }
    if (args.positionals().size() > 1) {
      throw new IllegalArgumentException("remove accepts one dependency name");
    }
    return new CommandLine.Remove(args.positionals().getFirst(), args.flag("test"));
  }
}
