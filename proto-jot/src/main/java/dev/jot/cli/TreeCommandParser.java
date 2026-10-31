package dev.jot.cli;

/** Parses direct and workspace dependency tree commands. */
final class TreeCommandParser {
  private TreeCommandParser() {}

  static CommandLine.Tree parse(CommandArguments args) {
    args.allow("module", "depth", "workspace");
    if (args.positionals().size() > 1) {
      throw new IllegalArgumentException("tree accepts one dependency coordinate");
    }
    int depth = 3;
    if (args.value("depth") != null) {
      try {
        depth = Integer.parseInt(args.value("depth"));
      } catch (NumberFormatException error) {
        throw new IllegalArgumentException("--depth needs a non-negative integer");
      }
      if (depth < 0) throw new IllegalArgumentException("--depth needs a non-negative integer");
    }
    String coordinate = args.positionals().isEmpty() ? null : args.positionals().getFirst();
    boolean workspace = args.flag("workspace");
    if (coordinate == null && !workspace) {
      throw new IllegalArgumentException("tree needs a dependency coordinate or --workspace");
    }
    if (coordinate != null && workspace) {
      throw new IllegalArgumentException("tree dependency cannot be combined with --workspace");
    }
    return new CommandLine.Tree(args.value("module"), coordinate, depth, workspace);
  }
}
