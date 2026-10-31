package dev.jot.cli;

import java.nio.file.Path;
import java.util.List;

/** Parsed global options and one command with only the options that command accepts. */
record CommandLine(
    boolean help, boolean version, boolean offline, Path projectRoot, Command command) {
  sealed interface Command
      permits Build,
          Run,
          Test,
          Doc,
          Lint,
          Fmt,
          Deps,
          Outdated,
          Add,
          Remove,
          Lock,
          Clean,
          Init,
          Tree,
          Resolve,
          Bench,
          Toolchain {}

  record Build(String module) implements Command {}

  record Run(String module, List<String> arguments) implements Command {}

  record Test(String module) implements Command {}

  record Doc(String module) implements Command {}

  record Lint(String module) implements Command {}

  record Fmt(String module, boolean check) implements Command {}

  record Deps(String module) implements Command {}

  record Outdated(String module) implements Command {}

  record Add(String coordinate, String name, String catalog, boolean test) implements Command {}

  record Remove(String name, boolean test) implements Command {}

  record Lock() implements Command {}

  record Clean() implements Command {}

  record Init(String name, String template, String packageName) implements Command {}

  record Tree(String module, String coordinate, int depth, boolean workspace) implements Command {}

  record Resolve(String coordinate, boolean dependencies) implements Command {}

  record Bench(String filter, String module, int forks, Integer warmup, Integer iterations)
      implements Command {}

  record Toolchain(ToolchainAction action) implements Command {}

  sealed interface ToolchainAction permits ListToolchains, PinToolchain, InstallToolchain {}

  record ListToolchains() implements ToolchainAction {}

  record PinToolchain(String specification, boolean workspace) implements ToolchainAction {}

  record InstallToolchain(String specification, boolean force) implements ToolchainAction {}
}
