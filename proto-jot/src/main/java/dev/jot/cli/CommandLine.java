package dev.jot.cli;

import java.nio.file.Path;
import java.util.List;

/** Parsed command and options accepted by the Jot CLI. */
record CommandLine(
    String command,
    Path projectRoot,
    String module,
    List<String> runArguments,
    boolean help,
    boolean version,
    boolean offline,
    String projectName,
    String template,
    String packageName,
    String dependencyCoordinate,
    String dependencyName,
    String dependencyCatalog,
    String toolchainAction,
    String toolchainSpec,
    boolean toolchainForce,
    boolean resolveDependencies,
    boolean testDependency,
    int treeDepth,
    boolean treeWorkspace,
    boolean workspacePin,
    boolean formatCheck) {}
