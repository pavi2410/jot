package dev.jot.cli;

/** Parses self-management subcommands. */
final class SelfCommandParser {
  private SelfCommandParser() {}

  static CommandLine.Self parse(CommandArguments args) {
    if (args.projectRoot() != null)
      throw new IllegalArgumentException("--project does not apply to self commands");
    if (args.positionals().isEmpty())
      throw new IllegalArgumentException("self needs update or uninstall");
    String action = args.positionals().getFirst();
    if (args.positionals().size() != 1)
      throw new IllegalArgumentException("unexpected self argument: " + args.positionals().get(1));
    return switch (action) {
      case "update" -> {
        args.allow("check", "yes", "version");
        yield new CommandLine.Self(
            new CommandLine.SelfUpdate(
                args.value("version"), args.flag("check"), args.flag("yes")));
      }
      case "uninstall" -> {
        args.allow("yes");
        yield new CommandLine.Self(new CommandLine.SelfUninstall(args.flag("yes")));
      }
      default -> throw new IllegalArgumentException("self needs update or uninstall");
    };
  }
}
