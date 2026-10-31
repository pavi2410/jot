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
          "package");
  private static final java.util.Set<String> FLAG_OPTIONS =
      java.util.Set.of("check", "deps", "fix", "ci", "workspace", "force", "test", "global");

  private CommandLineParser() {}

  static CommandLine parse(String[] argv) {
    var tokens = new ArrayList<>(java.util.List.of(argv));
    boolean offline = false;
    while (!tokens.isEmpty() && tokens.getFirst().equals("--offline")) {
      offline = true;
      tokens.removeFirst();
    }
    if (tokens.isEmpty() || tokens.getFirst().equals("--help") || tokens.getFirst().equals("-h"))
      return special(true, false);
    if (tokens.getFirst().equals("--version") || tokens.getFirst().equals("-V"))
      return special(false, true);

    String command = tokens.removeFirst();
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
          case "toolchain" -> ToolchainCommandParser.parse(args);
          case "build", "run", "test", "doc", "lint", "fmt", "deps", "outdated", "lock", "clean" ->
              SimpleCommandParser.parse(command, args);
          default -> throw new IllegalArgumentException("unknown command: " + command);
        };
    return new CommandLine(false, false, offline, project, parsed);
  }

  private static CommandLine special(boolean help, boolean version) {
    return new CommandLine(help, version, false, null, null);
  }
}
