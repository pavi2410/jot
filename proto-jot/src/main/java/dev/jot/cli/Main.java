package dev.jot.cli;

import dev.jot.core.BuildEngine;
import dev.jot.core.ProjectConfig;
import dev.jot.core.ProjectWorkspace;
import dev.jot.core.WorkspaceProject;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/** Command-line entry point for the Java Jot implementation. */
public final class Main {
    private static final String VERSION = "0.2.0-dev";

    private Main() {}

    public static void main(String[] args) {
        try {
            execute(args);
        } catch (Exception error) {
            System.err.println("error: " + error.getMessage());
            System.exit(1);
        }
    }

    private static void execute(String[] rawArguments) throws Exception {
        CommandLine commandLine = CommandLine.parse(rawArguments);
        if (commandLine.help()) {
            help();
            return;
        }
        if (commandLine.version()) {
            System.out.println("jot " + VERSION);
            return;
        }

        Path projectRoot = commandLine.projectRoot();
        if (projectRoot == null) projectRoot = findProjectRoot(Path.of("").toAbsolutePath());
        ProjectConfig config = ProjectConfig.read(projectRoot.resolve("jot.toml"), null);
        ProjectWorkspace workspace = config.isWorkspace()
                ? ProjectWorkspace.load(projectRoot, config)
                : ProjectWorkspace.loadProject(projectRoot, config);
        switch (commandLine.command()) {
            case "build" -> build(config, workspace, commandLine.module());
            case "run" -> run(config, workspace, commandLine.module(), commandLine.runArguments());
            case "clean" -> {
                for (WorkspaceProject project : workspace.ordered()) BuildEngine.clean(project);
            }
            default -> throw new IllegalArgumentException("unknown command: " + commandLine.command());
        }
    }

    private static void build(ProjectConfig config, ProjectWorkspace workspace, String module)
            throws Exception {
        if (!config.isWorkspace() && module != null) {
            throw new IllegalArgumentException("--module requires a workspace");
        }
        List<WorkspaceProject> projects = module == null && config.isWorkspace()
                ? workspace.ordered()
                : workspace.dependenciesFirst(workspace.find(module == null ? config.name() : module));
        for (WorkspaceProject project : projects) BuildEngine.build(project);
    }

    private static void run(ProjectConfig config, ProjectWorkspace workspace, String module,
                            List<String> runArguments) throws Exception {
        if (!config.isWorkspace() && module != null) {
            throw new IllegalArgumentException("--module requires a workspace");
        }
        WorkspaceProject project = config.isWorkspace()
                ? workspace.selectRunnable(module) : workspace.find(config.name());
        if (project.config().mainClass().isBlank()) {
            throw new IllegalArgumentException("project.main-class is required for `jot run`");
        }
        for (WorkspaceProject dependency : workspace.dependenciesFirst(project)) {
            BuildEngine.build(dependency);
        }
        BuildEngine.run(project, runArguments);
    }

    private static void help() {
        System.out.println("jot " + VERSION + " — a Java build tool");
        System.out.println("Usage: jot <build|run|clean> [--project <directory>] [--module <name>] [-- <args...>]");
    }

    private static Path findProjectRoot(Path start) {
        for (Path current = start; current != null; current = current.getParent()) {
            if (Files.isRegularFile(current.resolve("jot.toml"))) return current;
        }
        throw new IllegalArgumentException("could not find jot.toml from " + start);
    }
}
