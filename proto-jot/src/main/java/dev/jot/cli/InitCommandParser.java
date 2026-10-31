package dev.jot.cli;

/** Parses project creation options. */
final class InitCommandParser {
  private InitCommandParser() {}

  static CommandLine.Init parse(CommandArguments args) {
    args.allow("template", "package");
    if (args.positionals().size() > 1) {
      throw new IllegalArgumentException("init accepts one project name");
    }
    return new CommandLine.Init(
        args.positionals().isEmpty() ? null : args.positionals().getFirst(),
        args.value("template"),
        args.value("package"));
  }
}
