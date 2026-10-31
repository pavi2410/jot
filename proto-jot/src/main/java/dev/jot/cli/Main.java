package dev.jot.cli;

import dev.jot.core.BuildEngine;
import dev.jot.core.DependencyTree;
import dev.jot.core.DependencyListing;
import dev.jot.core.ProjectConfig;
import dev.jot.core.ProjectInitializer;
import dev.jot.core.ProjectWorkspace;
import dev.jot.core.WorkspaceProject;
import dev.jot.core.config.ManifestEditor;
import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
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
            System.err.println("error: " + failureMessage(error));
            System.exit(1);
        }
    }

    static String failureMessage(Throwable error) {
        return error.getMessage() == null || error.getMessage().isBlank()
                ? error.toString() : error.getMessage();
    }

    static String inferredProjectName(Path destination) {
        Path fileName = destination.toAbsolutePath().normalize().getFileName();
        String name = fileName == null ? "jot-project" : fileName.toString()
                .replaceAll("[^A-Za-z0-9_-]+", "-").replaceAll("-+", "-");
        if (name.isBlank()) return "jot-project";
        if (!Character.isLetter(name.charAt(0))) name = "project-" + name;
        return name;
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

        if (commandLine.command().equals("init")) {
            if (commandLine.projectRoot() != null && commandLine.projectName() != null) {
                throw new IllegalArgumentException("use either the init name or --project, not both");
            }
            Path destination = commandLine.projectRoot();
            if (destination == null) {
                Path current = Path.of("").toAbsolutePath();
                destination = commandLine.projectName() == null ? current
                        : current.resolve(commandLine.projectName());
            }
            String name = commandLine.projectName();
            if (name == null) name = inferredProjectName(destination);
            Path created = ProjectInitializer.initialize(destination, name,
                    commandLine.template(), commandLine.packageName());
            System.out.println("init: created "
                    + (commandLine.template() == null ? "app" : commandLine.template())
                    + " project at " + created);
            return;
        }

        if (commandLine.command().equals("tree")) {
            MavenResolver resolver = new MavenResolver(
                    Path.of(System.getProperty("user.home"), ".jot", "java-maven-cache"),
                    MavenResolver.defaultRepository(), commandLine.offline());
            List<MavenResolver.TreeEntry> entries = resolver.resolveDependencyTree(
                    MavenCoordinate.parse(commandLine.dependencyCoordinate()), commandLine.treeDepth());
            System.out.print(DependencyTree.render(entries));
            return;
        }

        Path projectRoot = commandLine.projectRoot();
        if (projectRoot == null) projectRoot = findProjectRoot(Path.of("").toAbsolutePath());
        ProjectConfig config = ProjectConfig.read(projectRoot.resolve("jot.toml"), null);
        ProjectWorkspace workspace = config.isWorkspace()
                ? ProjectWorkspace.load(projectRoot, config)
                : ProjectWorkspace.loadProject(projectRoot, config);
        MavenResolver resolver = new MavenResolver(
                Path.of(System.getProperty("user.home"), ".jot", "java-maven-cache"),
                MavenResolver.defaultRepository(), commandLine.offline());
        switch (commandLine.command()) {
            case "build" -> build(config, workspace, commandLine.module(), resolver);
            case "run" -> run(config, workspace, commandLine.module(), commandLine.runArguments(), resolver);
            case "test" -> test(config, workspace, commandLine.module(), resolver);
            case "doc" -> doc(config, workspace, commandLine.module(), resolver);
            case "deps" -> System.out.print(DependencyListing.render(config, workspace,
                    commandLine.module(), projectRoot.resolve("jot.lock")));
            case "outdated" -> System.out.print(DependencyListing.renderOutdated(config, workspace,
                    commandLine.module(), projectRoot.resolve("jot.lock"), resolver));
            case "add" -> addDependency(projectRoot, commandLine, resolver);
            case "remove" -> removeDependency(projectRoot, commandLine, resolver);
            case "lock" -> BuildEngine.lock(workspace, projectRoot.resolve("jot.lock"), resolver);
            case "clean" -> {
                for (WorkspaceProject project : workspace.ordered()) BuildEngine.clean(project);
            }
            default -> throw new IllegalArgumentException("unknown command: " + commandLine.command());
        }
    }

    private static void addDependency(Path projectRoot, CommandLine commandLine,
                                      MavenResolver resolver) throws Exception {
        MavenCoordinate coordinate = commandLine.dependencyCoordinate() == null ? null
                : MavenCoordinate.parse(commandLine.dependencyCoordinate());
        String name = commandLine.dependencyName() != null ? commandLine.dependencyName()
                : coordinate == null ? commandLine.dependencyCatalog() : coordinate.artifact();
        Path manifest = projectRoot.resolve("jot.toml");
        if (coordinate == null) {
            ManifestEditor.addCatalogDependency(manifest, name, commandLine.dependencyCatalog(),
                    commandLine.testDependency());
        } else {
            ManifestEditor.addDependency(manifest, name, coordinate.notation(),
                    commandLine.testDependency());
        }
        ProjectConfig updatedConfig = ProjectConfig.read(manifest, null);
        ProjectWorkspace updatedWorkspace = updatedConfig.isWorkspace()
                ? ProjectWorkspace.load(projectRoot, updatedConfig)
                : ProjectWorkspace.loadProject(projectRoot, updatedConfig);
        BuildEngine.lock(updatedWorkspace, projectRoot.resolve("jot.lock"), resolver);
        System.out.println("add: " + name + " -> " + manifest);
    }

    private static void removeDependency(Path projectRoot, CommandLine commandLine,
                                         MavenResolver resolver) throws Exception {
        String name = commandLine.dependencyName();
        Path manifest = projectRoot.resolve("jot.toml");
        if (!ManifestEditor.removeDependency(manifest, name, commandLine.testDependency())) {
            throw new IllegalArgumentException("dependency `" + name + "` was not found in ["
                    + (commandLine.testDependency() ? "test-dependencies" : "dependencies") + "]");
        }
        ProjectConfig updatedConfig = ProjectConfig.read(manifest, null);
        ProjectWorkspace updatedWorkspace = updatedConfig.isWorkspace()
                ? ProjectWorkspace.load(projectRoot, updatedConfig)
                : ProjectWorkspace.loadProject(projectRoot, updatedConfig);
        BuildEngine.lock(updatedWorkspace, projectRoot.resolve("jot.lock"), resolver);
        System.out.println("remove: " + name + " from " + manifest);
    }

    private static void build(ProjectConfig config, ProjectWorkspace workspace, String module,
                              MavenResolver resolver)
            throws Exception {
        if (!config.isWorkspace() && module != null) {
            throw new IllegalArgumentException("--module requires a workspace");
        }
        List<WorkspaceProject> projects = module == null && config.isWorkspace()
                ? workspace.ordered()
                : workspace.dependenciesFirst(workspace.find(module == null ? config.name() : module));
        for (WorkspaceProject project : projects) BuildEngine.build(project, resolver);
    }

    private static void run(ProjectConfig config, ProjectWorkspace workspace, String module,
                            List<String> runArguments, MavenResolver resolver) throws Exception {
        if (!config.isWorkspace() && module != null) {
            throw new IllegalArgumentException("--module requires a workspace");
        }
        WorkspaceProject project = config.isWorkspace()
                ? workspace.selectRunnable(module) : workspace.find(config.name());
        if (project.config().mainClass().isBlank()) {
            throw new IllegalArgumentException("project.main-class is required for `jot run`");
        }
        for (WorkspaceProject dependency : workspace.dependenciesFirst(project)) {
            BuildEngine.build(dependency, resolver);
        }
        BuildEngine.run(project, runArguments, resolver);
    }

    private static void test(ProjectConfig config, ProjectWorkspace workspace, String module,
                             MavenResolver resolver)
            throws Exception {
        if (!config.isWorkspace() && module != null) {
            throw new IllegalArgumentException("--module requires a workspace");
        }
        List<WorkspaceProject> projects = module == null && config.isWorkspace()
                ? workspace.ordered()
                : workspace.dependenciesFirst(workspace.find(module == null ? config.name() : module));
        for (WorkspaceProject project : projects) BuildEngine.test(project, resolver);
    }

    private static void doc(ProjectConfig config, ProjectWorkspace workspace, String module,
                            MavenResolver resolver) throws Exception {
        if (!config.isWorkspace() && module != null) {
            throw new IllegalArgumentException("--module requires a workspace");
        }
        List<WorkspaceProject> projects = module == null && config.isWorkspace()
                ? workspace.ordered()
                : List.of(workspace.find(module == null ? config.name() : module));
        java.util.Set<Path> built = new java.util.HashSet<>();
        for (WorkspaceProject project : projects) {
            for (WorkspaceProject dependency : workspace.dependenciesFirst(project)) {
                if (!dependency.root().equals(project.root()) && built.add(dependency.root())) {
                    BuildEngine.build(dependency, resolver);
                }
            }
            Path docs = BuildEngine.doc(project, resolver);
            System.out.println("doc: " + project.name() + " -> " + docs);
        }
    }

    private static void help() {
        System.out.println("jot " + VERSION + " — a Java build tool");
        System.out.println("Usage: jot <init|build|run|test|doc|add|remove|deps|outdated|tree|lock|clean> [options]");
        System.out.println("       jot add <group:artifact:version> [--name <alias>] [--test]");
        System.out.println("       jot add --catalog <alias> [--name <dependency>] [--test]");
        System.out.println("       jot remove <alias> [--test]");
        System.out.println("       jot deps [--module <name>]");
        System.out.println("       jot outdated [--module <name>] [--offline]");
        System.out.println("       jot tree <group:artifact:version> [--depth <levels>] [--offline]");
        System.out.println("       jot doc [--project <directory>] [--module <name>] [--offline]");
        System.out.println("       jot init [name] [--template app|library] [--package java.package]");
        System.out.println("       jot <build|run|test|lock|clean> [--project <directory>] [--module <name>] [--offline]");
    }

    private static Path findProjectRoot(Path start) {
        for (Path current = start; current != null; current = current.getParent()) {
            if (Files.isRegularFile(current.resolve("jot.toml"))) return current;
        }
        throw new IllegalArgumentException("could not find jot.toml from " + start);
    }
}
