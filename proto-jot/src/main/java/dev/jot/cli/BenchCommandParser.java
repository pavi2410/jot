package dev.jot.cli;

/** Parses benchmark filters and JMH iteration options. */
final class BenchCommandParser {
  private BenchCommandParser() {}

  static CommandLine.Bench parse(CommandArguments args) {
    args.allow("module", "forks", "warmup", "iterations");
    if (args.positionals().size() > 1) {
      throw new IllegalArgumentException("bench accepts one filter");
    }
    String filter = args.positionals().isEmpty() ? null : args.positionals().getFirst();
    if (filter != null && filter.isBlank()) {
      throw new IllegalArgumentException("bench filter cannot be blank");
    }
    return new CommandLine.Bench(
        filter,
        args.value("module"),
        count(args, "forks", 1),
        optionalCount(args, "warmup"),
        optionalCount(args, "iterations"));
  }

  private static Integer optionalCount(CommandArguments args, String name) {
    return args.value(name) == null ? null : count(args, name, 0);
  }

  private static int count(CommandArguments args, String name, int defaultValue) {
    String value = args.value(name);
    if (value == null) return defaultValue;
    try {
      int count = Integer.parseInt(value);
      if (count >= 0) return count;
    } catch (NumberFormatException ignored) {
      // Use one consistent error for malformed and negative counts.
    }
    throw new IllegalArgumentException("--" + name + " needs a non-negative integer");
  }
}
