package dev.jot.cli;

/** Parses project and ad hoc dependency lock commands. */
final class LockCommandParser {
  private LockCommandParser() {}

  static CommandLine.Lock parse(CommandArguments args) {
    args.allow("depth", "output");
    int depth = 8;
    if (args.value("depth") != null) {
      try {
        depth = Integer.parseInt(args.value("depth"));
      } catch (NumberFormatException error) {
        throw new IllegalArgumentException("--depth needs a non-negative integer");
      }
      if (depth < 0) throw new IllegalArgumentException("--depth needs a non-negative integer");
    }
    return new CommandLine.Lock(
        args.positionals(),
        depth,
        args.value("output") == null ? "jot.lock" : args.value("output"));
  }
}
