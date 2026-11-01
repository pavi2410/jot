package dev.jot.cli;

/** Parses Maven repository publication options. */
final class PublishCommandParser {
  private PublishCommandParser() {}

  static CommandLine.Publish parse(CommandArguments args) {
    args.allow(
        "module", "repository", "username", "password", "signing-key", "dry-run", "allow-snapshot");
    args.noPositionals("publish");
    return new CommandLine.Publish(
        args.value("module"),
        args.value("repository"),
        args.value("username"),
        args.value("password"),
        args.value("signing-key"),
        args.flag("dry-run"),
        args.flag("allow-snapshot"));
  }
}
