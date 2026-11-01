package dev.jot.cli;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;

/** Tokenizes global options, then delegates command-specific parsing. */
final class CommandLineParser {
  private static final java.util.Set<String> VALUE_OPTIONS =
      java.util.Set.of(
          "module",
          "depth",
          "forks",
          "warmup",
          "iterations",
          "name",
          "catalog",
          "template",
          "group",
          "repository",
          "username",
          "password",
          "signing-key",
          "output",
          "version",
          "package");
  private static final java.util.Set<String> FLAG_OPTIONS =
      java.util.Set.of(
          "check",
          "open",
          "deps",
          "fix",
          "ci",
          "workspace",
          "force",
          "yes",
          "test",
          "global",
          "clean",
          "native",
          "watch",
          "dry-run",
          "allow-snapshot");

  private CommandLineParser() {}

  static CommandLine parse(String[] argv) {
    var tokens = new ArrayList<>(java.util.List.of(argv));
    boolean offline = false;
    while (!tokens.isEmpty() && tokens.getFirst().equals("--offline")) {
      offline = true;
      tokens.removeFirst();
    }
    if (tokens.isEmpty() || tokens.getFirst().equals("--help") || tokens.getFirst().equals("-h"))
      return help("");
    if (tokens.getFirst().equals("--version") || tokens.getFirst().equals("-V"))
      return special(false, true);

    String command = tokens.getFirst();
    int helpIndex = helpIndex(tokens);
    if (helpIndex >= 0) return help(helpTopic(command, tokens.subList(1, helpIndex)));
    tokens.removeFirst();
    Path project = null;
    var positionals = new ArrayList<String>();
    var runArguments = new ArrayList<String>();
    var values = new HashMap<String, String>();
    var flags = new HashSet<String>();
    for (int i = 0; i < tokens.size(); i++) {
      String token = tokens.get(i);
      if (token.equals("--") && command.equals("run")) {
        runArguments.addAll(tokens.subList(i + 1, tokens.size()));
        break;
      }
      if (!token.startsWith("--")) {
        positionals.add(token);
        continue;
      }
      String option = token.substring(2);
      if (option.equals("offline")) {
        offline = true;
      } else if (option.equals("project")) {
        if (++i >= tokens.size() || tokens.get(i).equals("--"))
          throw new IllegalArgumentException("--project needs a value");
        project = Path.of(tokens.get(i)).toAbsolutePath().normalize();
      } else if (VALUE_OPTIONS.contains(option)) {
        if (++i >= tokens.size() || tokens.get(i).equals("--"))
          throw new IllegalArgumentException("--" + option + " needs a value");
        if (values.putIfAbsent(option, tokens.get(i)) != null)
          throw new IllegalArgumentException("--" + option + " may only be supplied once");
      } else if (FLAG_OPTIONS.contains(option)) {
        if (!flags.add(option))
          throw new IllegalArgumentException("--" + option + " may only be supplied once");
      } else {
        throw new IllegalArgumentException("unknown argument: " + token);
      }
    }
    CommandArguments args =
        new CommandArguments(project, offline, positionals, runArguments, values, flags);
    CommandLine.Command parsed =
        switch (command) {
          case "init" -> InitCommandParser.parse(args);
          case "add", "remove" -> DependencyCommandParser.parse(command, args);
          case "tree" -> TreeCommandParser.parse(args);
          case "resolve" -> ResolveCommandParser.parse(args);
          case "bench" -> BenchCommandParser.parse(args);
          case "audit" -> AuditCommandParser.parse(args);
          case "publish" -> PublishCommandParser.parse(args);
          case "lock" -> LockCommandParser.parse(args);
          case "toolchain" -> ToolchainCommandParser.parse(args);
          case "self" -> SelfCommandParser.parse(args);
          case "build", "run", "test", "doc", "lint", "fmt", "deps", "outdated", "clean" ->
              SimpleCommandParser.parse(command, args);
          default -> throw new IllegalArgumentException("unknown command: " + command);
        };
    return new CommandLine(false, false, offline, project, parsed, null);
  }

  private static int helpIndex(java.util.List<String> tokens) {
    for (int index = 1; index < tokens.size(); index++) {
      String token = tokens.get(index);
      if (token.equals("--")) return -1;
      if (token.equals("--help") || token.equals("-h")) return index;
    }
    return -1;
  }

  private static String helpTopic(String command, java.util.List<String> precedingTokens) {
    if (!command.equals("toolchain") && !command.equals("self")) return command;
    String action = firstPositional(precedingTokens);
    String nested = action == null ? command : command + " " + action;
    return HelpPrinter.hasTopic(nested) ? nested : command;
  }

  private static String firstPositional(java.util.List<String> tokens) {
    for (int index = 0; index < tokens.size(); index++) {
      String token = tokens.get(index);
      if (token.equals("--")) return null;
      if (token.equals("--project")
          || VALUE_OPTIONS.contains(token.startsWith("--") ? token.substring(2) : "")) {
        index++;
      } else if (!token.startsWith("-")) {
        return token;
      }
    }
    return null;
  }

  private static CommandLine help(String topic) {
    return new CommandLine(true, false, false, null, null, topic);
  }

  private static CommandLine special(boolean help, boolean version) {
    return new CommandLine(help, version, false, null, null, help ? "" : null);
  }
}
