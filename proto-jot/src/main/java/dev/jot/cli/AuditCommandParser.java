package dev.jot.cli;

/** Parses dependency vulnerability audit options. */
final class AuditCommandParser {
  private AuditCommandParser() {}

  static CommandLine.Audit parse(CommandArguments args) {
    args.allow("fix", "ci");
    args.noPositionals("audit");
    return new CommandLine.Audit(args.flag("fix"), args.flag("ci"));
  }
}
