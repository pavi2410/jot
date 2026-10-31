package dev.jot.cli;

import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** Tokenized options and positional values passed to one subcommand parser. */
record CommandArguments(
    Path projectRoot,
    boolean offline,
    List<String> positionals,
    List<String> runArguments,
    Map<String, String> values,
    Set<String> flags) {
  CommandArguments {
    positionals = List.copyOf(positionals);
    runArguments = List.copyOf(runArguments);
    values = Map.copyOf(values);
    flags = Set.copyOf(flags);
  }

  String value(String option) {
    return values.get(option);
  }

  boolean flag(String option) {
    return flags.contains(option);
  }

  void allow(String... options) {
    Set<String> allowed = Set.of(options);
    values.keySet().stream()
        .filter(option -> !allowed.contains(option))
        .findFirst()
        .ifPresent(option -> unknown(option));
    flags.stream()
        .filter(option -> !allowed.contains(option))
        .findFirst()
        .ifPresent(option -> unknown(option));
  }

  void noPositionals(String command) {
    if (!positionals.isEmpty())
      throw new IllegalArgumentException("unknown argument: " + positionals.getFirst());
  }

  String onePositional(String command, String error) {
    if (positionals.isEmpty()) return null;
    if (positionals.size() > 1) throw new IllegalArgumentException(error);
    return positionals.getFirst();
  }

  static void unknown(String option) {
    throw new IllegalArgumentException("unknown argument: --" + option);
  }
}
