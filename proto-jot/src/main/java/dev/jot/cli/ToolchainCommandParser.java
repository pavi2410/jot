package dev.jot.cli;

import java.util.List;

/** Parses toolchain subcommands and their options. */
final class ToolchainCommandParser {
  private ToolchainCommandParser() {}

  static CommandLine.Toolchain parse(CommandArguments args) {
    args.allow("workspace", "force");
    if (args.positionals().isEmpty())
      throw new IllegalArgumentException("toolchain needs list, pin, or install");
    String action = args.positionals().getFirst();
    if (!List.of("list", "pin", "install").contains(action))
      throw new IllegalArgumentException("unknown toolchain command: " + action);
    int expected = action.equals("list") ? 1 : 2;
    if (args.positionals().size() != expected)
      throw new IllegalArgumentException(
          "toolchain " + action + " needs java@<version|vendor-version>");
    String spec = expected == 2 ? args.positionals().get(1) : null;
    boolean workspace = args.flag("workspace");
    boolean force = args.flag("force");
    if (workspace && !action.equals("pin"))
      throw new IllegalArgumentException("--workspace only applies to toolchain pin");
    if (force && !action.equals("install"))
      throw new IllegalArgumentException("--force only applies to toolchain install");
    CommandLine.ToolchainAction parsed =
        switch (action) {
          case "list" -> new CommandLine.ListToolchains();
          case "pin" -> new CommandLine.PinToolchain(spec, workspace);
          case "install" -> new CommandLine.InstallToolchain(spec, force);
          default -> throw new IllegalStateException(action);
        };
    return new CommandLine.Toolchain(parsed);
  }
}
