package dev.jot.cli;

/** Parses a pinned Maven coordinate resolution command. */
final class ResolveCommandParser {
  private ResolveCommandParser() {}

  static CommandLine.Resolve parse(CommandArguments args) {
    args.allow("deps");
    if (args.positionals().isEmpty()) {
      throw new IllegalArgumentException("resolve needs a dependency coordinate");
    }
    if (args.positionals().size() > 1) {
      throw new IllegalArgumentException("resolve accepts one dependency coordinate");
    }
    return new CommandLine.Resolve(args.positionals().getFirst(), args.flag("deps"));
  }
}
