package dev.jot.cli;

import dev.jot.core.BuildEngine;
import dev.jot.core.DependencyAuditor;
import dev.jot.core.DependencyListing;
import dev.jot.core.DependencyTree;
import dev.jot.core.GlobalCache;
import dev.jot.core.JavaToolchain;
import dev.jot.core.JavaToolchainManager;
import dev.jot.core.MavenPublisher;
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
  private static final String VERSION = version();

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
        ? error.toString()
        : error.getMessage();
  }

  static String inferredProjectName(Path destination) {
    Path fileName = destination.toAbsolutePath().normalize().getFileName();
    String name =
        fileName == null
            ? "jot-project"
            : fileName.toString().replaceAll("[^A-Za-z0-9_-]+", "-").replaceAll("-+", "-");
    if (name.isBlank()) return "jot-project";
    if (!Character.isLetter(name.charAt(0))) name = "project-" + name;
    return name;
  }

  static void execute(String[] rawArguments) throws Exception {
    CommandLine commandLine = CommandLineParser.parse(rawArguments);
    if (commandLine.help()) {
      System.out.print(HelpPrinter.render(commandLine.helpTopic(), VERSION));
      return;
    }
    if (commandLine.version()) {
      System.out.println("jot " + VERSION);
      return;
    }
    if (commandLine.command() instanceof CommandLine.Self self) {
      if (commandLine.offline() && self.action() instanceof CommandLine.SelfUpdate) {
        throw new IllegalArgumentException("self update requires network access");
      }
      SelfManager.execute(self, VERSION);
      return;
    }
    if (commandLine.command() instanceof CommandLine.Clean clean && clean.global()) {
      Path jotHome = Path.of(System.getProperty("user.home"), ".jot");
      GlobalCache.CleanupSummary summary = GlobalCache.clear(jotHome);
      System.out.println(
          "clean: removed "
              + summary.removedEntries()
              + " cache entries from "
              + jotHome.toAbsolutePath().normalize());
      return;
    }
    if (commandLine.command() instanceof CommandLine.Toolchain toolchain) {
      toolchain(toolchain, commandLine);
      return;
    }

    if (commandLine.command() instanceof CommandLine.Init init) {
      if (commandLine.projectRoot() != null && init.name() != null) {
        throw new IllegalArgumentException("use either the init name or --project, not both");
      }
      Path destination = commandLine.projectRoot();
      if (destination == null) {
        Path current = Path.of("").toAbsolutePath();
        destination = init.name() == null ? current : current.resolve(init.name());
      }
      String name = init.name();
      if (name == null) name = inferredProjectName(destination);
      Path created =
          ProjectInitializer.initialize(
              destination, name, init.template(), init.group(), init.packageName());
      System.out.println(
          "init: created "
              + (init.template() == null ? "app" : init.template())
              + " project at "
              + created);
      return;
    }

    if (commandLine.command() instanceof CommandLine.Tree tree) {
      MavenResolver resolver =
          new MavenResolver(
              Path.of(System.getProperty("user.home"), ".jot", "java-maven-cache"),
              MavenResolver.defaultRepository(),
              commandLine.offline());
      if (tree.workspace()) {
        Path root =
            commandLine.projectRoot() == null
                ? findProjectRoot(Path.of("").toAbsolutePath())
                : commandLine.projectRoot();
        ProjectConfig config = ProjectConfig.read(root.resolve("jot.toml"), null);
        if (!config.isWorkspace()) {
          throw new IllegalArgumentException("--workspace requires a workspace jot.toml");
        }
        ProjectWorkspace workspace = ProjectWorkspace.load(root, config);
        List<WorkspaceProject> projects =
            tree.module() == null ? workspace.ordered() : List.of(workspace.find(tree.module()));
        List<DependencyTree.WorkspaceModule> modules = new java.util.ArrayList<>();
        for (WorkspaceProject project : projects) {
          List<List<MavenResolver.TreeEntry>> externalTrees = new java.util.ArrayList<>();
          for (String coordinate : project.config().dependencies().values()) {
            externalTrees.add(
                resolver.resolveDependencyTree(MavenCoordinate.parse(coordinate), tree.depth()));
          }
          modules.add(
              new DependencyTree.WorkspaceModule(
                  project.name(),
                  project.dependencies().stream().map(WorkspaceProject::name).toList(),
                  externalTrees));
        }
        System.out.print(DependencyTree.renderWorkspace(modules));
        return;
      }
      List<MavenResolver.TreeEntry> entries =
          resolver.resolveDependencyTree(MavenCoordinate.parse(tree.coordinate()), tree.depth());
      System.out.print(DependencyTree.render(entries));
      return;
    }

    if (commandLine.command() instanceof CommandLine.Resolve resolve) {
      MavenResolver resolver =
          new MavenResolver(
              Path.of(System.getProperty("user.home"), ".jot", "java-maven-cache"),
              MavenResolver.defaultRepository(),
              commandLine.offline());
      MavenCoordinate coordinate = resolver.resolveCoordinate(resolve.coordinate());
      System.out.println("resolve: " + coordinate.notation());
      if (resolve.dependencies()) {
        List<MavenResolver.DirectDependency> dependencies =
            resolver.resolveDirectDependencies(coordinate);
        if (dependencies.isEmpty()) {
          System.out.println("deps: no direct dependencies");
        } else {
          for (MavenResolver.DirectDependency dependency : dependencies) {
            String optional = dependency.optional() ? " optional" : "";
            System.out.println(
                "dep: "
                    + dependency.coordinate().notation()
                    + " ["
                    + dependency.scope()
                    + optional
                    + "]");
          }
        }
      }
      return;
    }

    if (commandLine.command() instanceof CommandLine.Lock lock && !lock.dependencies().isEmpty()) {
      Path outputRoot =
          commandLine.projectRoot() == null
              ? Path.of("").toAbsolutePath().normalize()
              : commandLine.projectRoot();
      lockCoordinates(outputRoot, lock, newResolver(commandLine.offline()));
      return;
    }

    Path projectRoot = commandLine.projectRoot();
    if (projectRoot == null) projectRoot = findProjectRoot(Path.of("").toAbsolutePath());
    ProjectConfig config = ProjectConfig.read(projectRoot.resolve("jot.toml"), null);
    ProjectWorkspace workspace =
        config.isWorkspace()
            ? ProjectWorkspace.load(projectRoot, config)
            : ProjectWorkspace.loadProject(projectRoot, config);
    MavenResolver resolver = newResolver(commandLine.offline());
    switch (commandLine.command()) {
      case CommandLine.Build build -> build(config, workspace, build.module(), resolver);
      case CommandLine.Run run -> run(config, workspace, run.module(), run.arguments(), resolver);
      case CommandLine.Test test -> test(config, workspace, test.module(), resolver);
      case CommandLine.Doc doc -> doc(config, workspace, doc.module(), doc.open(), resolver);
      case CommandLine.Lint lint -> lint(config, workspace, lint.module(), resolver);
      case CommandLine.Fmt fmt -> fmt(config, workspace, fmt.module(), fmt.check(), resolver);
      case CommandLine.Deps deps ->
          System.out.print(
              DependencyListing.render(
                  config, workspace, deps.module(), projectRoot.resolve("jot.lock")));
      case CommandLine.Outdated outdated ->
          System.out.print(
              DependencyListing.renderOutdated(
                  config, workspace, outdated.module(), projectRoot.resolve("jot.lock"), resolver));
      case CommandLine.Bench bench -> bench(config, workspace, bench, resolver);
      case CommandLine.Audit audit -> audit(projectRoot, config, workspace, audit, resolver);
      case CommandLine.Publish publish -> publish(config, workspace, publish, resolver);
      case CommandLine.Add add -> addDependency(projectRoot, add, resolver);
      case CommandLine.Remove remove -> removeDependency(projectRoot, remove, resolver);
      case CommandLine.Lock lock -> lock(projectRoot, workspace, lock, resolver);
      case CommandLine.Clean ignored -> {
        Path runningJar = SelfManager.runningJarIfPackaged();
        for (WorkspaceProject project : workspace.ordered()) {
          Path target = project.root().resolve("target").toAbsolutePath().normalize();
          if (runningJar != null && runningJar.startsWith(target)) {
            BuildEngine.clean(project, runningJar);
          } else {
            BuildEngine.clean(project);
          }
        }
      }
      default -> throw new IllegalStateException("command handled before project loading");
    }
  }

  private static String version() {
    try (var input = Main.class.getResourceAsStream("/jot-version.properties")) {
      if (input == null) return "0.2.0-dev";
      var properties = new java.util.Properties();
      properties.load(input);
      return properties.getProperty("version", "0.2.0-dev");
    } catch (Exception ignored) {
      return "0.2.0-dev";
    }
  }

  private static void lock(
      Path projectRoot,
      ProjectWorkspace workspace,
      CommandLine.Lock command,
      MavenResolver resolver)
      throws Exception {
    Path output = projectRoot.resolve(command.output()).normalize();
    if (command.dependencies().isEmpty()) {
      BuildEngine.lock(workspace, output, resolver, command.depth());
      return;
    }
    lockCoordinates(projectRoot, command, resolver);
  }

  private static void lockCoordinates(
      Path projectRoot, CommandLine.Lock command, MavenResolver resolver) throws Exception {
    Path output = projectRoot.resolve(command.output()).normalize();
    java.util.Map<String, String> roots = new java.util.LinkedHashMap<>();
    for (int index = 0; index < command.dependencies().size(); index++) {
      roots.put("dependency-" + index, command.dependencies().get(index));
    }
    List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(roots, command.depth());
    dev.jot.core.resolve.Lockfile.create(roots.values(), resolved).write(output);
    System.out.println("lock: wrote " + output);
  }

  private static MavenResolver newResolver(boolean offline) {
    return new MavenResolver(
        Path.of(System.getProperty("user.home"), ".jot", "java-maven-cache"),
        MavenResolver.defaultRepository(),
        offline);
  }

  private static void addDependency(
      Path projectRoot, CommandLine.Add commandLine, MavenResolver resolver) throws Exception {
    MavenCoordinate coordinate =
        commandLine.coordinate() == null ? null : MavenCoordinate.parse(commandLine.coordinate());
    String name =
        commandLine.name() != null
            ? commandLine.name()
            : coordinate == null ? commandLine.catalog() : coordinate.artifact();
    Path manifest = projectRoot.resolve("jot.toml");
    if (coordinate == null) {
      ManifestEditor.addCatalogDependency(
          manifest, name, commandLine.catalog(), commandLine.test());
    } else {
      ManifestEditor.addDependency(manifest, name, coordinate.notation(), commandLine.test());
    }
    ProjectConfig updatedConfig = ProjectConfig.read(manifest, null);
    ProjectWorkspace updatedWorkspace =
        updatedConfig.isWorkspace()
            ? ProjectWorkspace.load(projectRoot, updatedConfig)
            : ProjectWorkspace.loadProject(projectRoot, updatedConfig);
    BuildEngine.lock(updatedWorkspace, projectRoot.resolve("jot.lock"), resolver);
    System.out.println("add: " + name + " -> " + manifest);
  }

  private static void removeDependency(
      Path projectRoot, CommandLine.Remove commandLine, MavenResolver resolver) throws Exception {
    String name = commandLine.name();
    Path manifest = projectRoot.resolve("jot.toml");
    if (!ManifestEditor.removeDependency(manifest, name, commandLine.test())) {
      throw new IllegalArgumentException(
          "dependency `"
              + name
              + "` was not found in ["
              + (commandLine.test() ? "test-dependencies" : "dependencies")
              + "]");
    }
    ProjectConfig updatedConfig = ProjectConfig.read(manifest, null);
    ProjectWorkspace updatedWorkspace =
        updatedConfig.isWorkspace()
            ? ProjectWorkspace.load(projectRoot, updatedConfig)
            : ProjectWorkspace.loadProject(projectRoot, updatedConfig);
    BuildEngine.lock(updatedWorkspace, projectRoot.resolve("jot.lock"), resolver);
    System.out.println("remove: " + name + " from " + manifest);
  }

  private static void bench(
      ProjectConfig config,
      ProjectWorkspace workspace,
      CommandLine.Bench options,
      MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && options.module() != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    List<WorkspaceProject> projects =
        options.module() == null && config.isWorkspace()
            ? workspace.ordered()
            : List.of(workspace.find(options.module() == null ? config.name() : options.module()));
    java.util.Set<Path> built = new java.util.HashSet<>();
    for (WorkspaceProject project : projects) {
      for (WorkspaceProject dependency : workspace.dependenciesFirst(project)) {
        if (!dependency.root().equals(project.root()) && built.add(dependency.root())) {
          BuildEngine.build(dependency, resolver);
        }
      }
      BuildEngine.bench(
          project,
          resolver,
          options.filter(),
          options.forks(),
          options.warmup(),
          options.iterations());
    }
  }

  private static void audit(
      Path projectRoot,
      ProjectConfig config,
      ProjectWorkspace workspace,
      CommandLine.Audit options,
      MavenResolver resolver)
      throws Exception {
    DependencyAuditor.Report report =
        new DependencyAuditor().audit(projectRoot, config, workspace, resolver, options.fix());
    if (report.findings().isEmpty()) {
      System.out.println(
          "audit: no vulnerabilities found across " + report.packagesScanned() + " packages");
      return;
    }
    long critical =
        report.findings().stream()
            .filter(finding -> finding.severity() == DependencyAuditor.AuditSeverity.CRITICAL)
            .count();
    long high =
        report.findings().stream()
            .filter(finding -> finding.severity() == DependencyAuditor.AuditSeverity.HIGH)
            .count();
    System.out.println(
        "audit: packages="
            + report.packagesScanned()
            + " findings="
            + report.findings().size()
            + " critical="
            + critical
            + " high="
            + high);
    for (DependencyAuditor.Finding finding : report.findings()) {
      System.out.println(
          "\n"
              + finding.severity()
              + " "
              + finding.id()
              + " "
              + finding.packageCoordinate().notation());
      System.out.println("  summary: " + finding.summary());
      if (finding.fixedVersion() != null) {
        System.out.println("  fixed version: " + finding.fixedVersion());
      }
      if (!finding.members().isEmpty()) {
        System.out.println("  affected members: " + String.join(", ", finding.members()));
      }
      for (int index = 0; index < finding.chains().size(); index++) {
        System.out.println(
            "  path "
                + (index + 1)
                + ": "
                + String.join(
                    " -> ",
                    finding.chains().get(index).stream().map(MavenCoordinate::notation).toList()));
      }
    }
    if (options.fix()) {
      System.out.println("\nfix: updated " + report.fixedDependencies() + " direct dependencies");
    }
    if (options.ci()
        && report.findings().stream().anyMatch(finding -> finding.severity().failsCi())) {
      throw new IllegalStateException("audit failed CI severity threshold");
    }
  }

  private static void publish(
      ProjectConfig config,
      ProjectWorkspace workspace,
      CommandLine.Publish options,
      MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && options.module() != null)
      throw new IllegalArgumentException("--module can only be used from inside a workspace");
    List<WorkspaceProject> targets =
        config.isWorkspace()
            ? options.module() == null
                ? workspace.ordered()
                : List.of(workspace.find(options.module()))
            : List.of(workspace.find(config.name()));
    java.util.Set<Path> built = new java.util.LinkedHashSet<>();
    for (WorkspaceProject target : targets) {
      for (WorkspaceProject dependency : workspace.dependenciesFirst(target)) {
        if (built.add(dependency.root())) BuildEngine.build(dependency, resolver);
      }
    }
    String repository =
        options.repository() == null || options.repository().isBlank()
            ? System.getenv("JOT_PUBLISH_REPOSITORY")
            : options.repository();
    String username =
        options.username() == null ? System.getenv("JOT_PUBLISH_USERNAME") : options.username();
    String password =
        options.password() == null ? System.getenv("JOT_PUBLISH_PASSWORD") : options.password();
    MavenPublisher.ArtifactSigner signer = MavenPublisher.gpgSigner(options.signingKey());
    for (WorkspaceProject project : targets) {
      MavenPublisher.Result result =
          MavenPublisher.publish(
              project,
              resolver,
              repository,
              username,
              password,
              options.allowSnapshot(),
              options.dryRun(),
              signer);
      System.out.println(
          (options.dryRun() ? "dry-run: staged " : "publish: ")
              + result.coordinate().notation()
              + " -> "
              + result.stagingDirectory());
    }
  }

  private static void toolchain(CommandLine.Toolchain command, CommandLine globals)
      throws Exception {
    if (command.action() instanceof CommandLine.InstallToolchain install) {
      String spec = install.specification();
      if (!spec.startsWith("java@")) {
        throw new IllegalArgumentException("expected java@<version>, got " + spec);
      }
      String version = spec.substring("java@".length());
      JavaToolchain.ToolchainRequest request = JavaToolchain.parseRequest(version);
      JavaToolchain.Installation installation =
          new JavaToolchainManager()
              .install(request.specification(), install.force(), globals.offline());
      System.out.println(
          "install: java "
              + installation.vendor()
              + "-"
              + installation.version()
              + " -> "
              + installation.home());
      return;
    }
    if (command.action() instanceof CommandLine.ListToolchains) {
      Path jdks = Path.of(System.getProperty("user.home"), ".jot", "jdks");
      List<JavaToolchain.Installation> installed = JavaToolchain.installedJdks(jdks);
      if (installed.isEmpty()) {
        System.out.println("toolchain: no managed JDKs installed");
        return;
      }
      for (JavaToolchain.Installation installation : installed) {
        System.out.println(
            "java@"
                + (installation.vendor().equals("unknown") ? "" : installation.vendor() + "-")
                + installation.feature()
                + " ("
                + installation.version()
                + ") -> "
                + installation.home());
      }
      return;
    }

    CommandLine.PinToolchain pin = (CommandLine.PinToolchain) command.action();
    String spec = pin.specification();
    if (!spec.startsWith("java@")) {
      throw new IllegalArgumentException("expected java@<version>, got " + spec);
    }
    String version = spec.substring("java@".length());
    JavaToolchain.ToolchainRequest request = JavaToolchain.parseRequest(version);
    Path start =
        globals.projectRoot() == null ? Path.of("").toAbsolutePath() : globals.projectRoot();
    Path manifest =
        pin.workspace() ? findWorkspaceManifest(start) : findProjectRoot(start).resolve("jot.toml");
    ManifestEditor.pinJavaToolchain(manifest, request.version(), request.vendor());
    System.out.println("toolchain: pinned java@" + request.specification() + " in " + manifest);
  }

  private static Path findWorkspaceManifest(Path start) {
    Path project = findProjectRoot(start);
    for (Path current = project; current != null; current = current.getParent()) {
      Path manifest = current.resolve("jot.toml");
      if (!Files.isRegularFile(manifest)) continue;
      try {
        if (ProjectConfig.read(manifest, null).isWorkspace()) return manifest;
      } catch (Exception error) {
        throw new IllegalArgumentException("cannot read workspace manifest " + manifest, error);
      }
    }
    throw new IllegalArgumentException("--workspace requires a workspace jot.toml");
  }

  private static void build(
      ProjectConfig config, ProjectWorkspace workspace, String module, MavenResolver resolver)
      throws Exception {
    long started = System.nanoTime();
    if (!config.isWorkspace() && module != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    List<WorkspaceProject> projects =
        module == null && config.isWorkspace()
            ? workspace.ordered()
            : workspace.dependenciesFirst(workspace.find(module == null ? config.name() : module));
    for (WorkspaceProject project : projects) BuildEngine.build(project, resolver);
    double seconds = (System.nanoTime() - started) / 1_000_000_000.0;
    System.out.printf(java.util.Locale.ROOT, "build: finished in %.3f s%n", seconds);
  }

  private static void run(
      ProjectConfig config,
      ProjectWorkspace workspace,
      String module,
      List<String> runArguments,
      MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && module != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    WorkspaceProject project =
        config.isWorkspace() ? workspace.selectRunnable(module) : workspace.find(config.name());
    if (project.config().mainClass().isBlank()) {
      throw new IllegalArgumentException("project.main-class is required for `jot run`");
    }
    for (WorkspaceProject dependency : workspace.dependenciesFirst(project)) {
      BuildEngine.build(dependency, resolver);
    }
    BuildEngine.run(project, runArguments, resolver);
  }

  private static void test(
      ProjectConfig config, ProjectWorkspace workspace, String module, MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && module != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    List<WorkspaceProject> projects =
        module == null && config.isWorkspace()
            ? workspace.ordered()
            : workspace.dependenciesFirst(workspace.find(module == null ? config.name() : module));
    for (WorkspaceProject project : projects) BuildEngine.test(project, resolver);
  }

  private static void doc(
      ProjectConfig config,
      ProjectWorkspace workspace,
      String module,
      boolean open,
      MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && module != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    List<WorkspaceProject> projects =
        module == null && config.isWorkspace()
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
      if (open) java.awt.Desktop.getDesktop().browse(docs.resolve("index.html").toUri());
    }
  }

  private static void lint(
      ProjectConfig config, ProjectWorkspace workspace, String module, MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && module != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    List<WorkspaceProject> projects =
        module == null && config.isWorkspace()
            ? workspace.ordered()
            : List.of(workspace.find(module == null ? config.name() : module));
    java.util.Set<Path> built = new java.util.HashSet<>();
    for (WorkspaceProject project : projects) {
      for (WorkspaceProject dependency : workspace.dependenciesFirst(project)) {
        if (!dependency.root().equals(project.root()) && built.add(dependency.root())) {
          BuildEngine.build(dependency, resolver);
        }
      }
      BuildEngine.LintResult result = BuildEngine.lint(project, resolver);
      if (!result.diagnostics().isBlank()) System.err.print(result.diagnostics());
      System.out.println(
          result.clean()
              ? "lint: " + project.name() + " is clean"
              : "lint: warnings in " + project.name());
    }
  }

  private static void fmt(
      ProjectConfig config,
      ProjectWorkspace workspace,
      String module,
      boolean check,
      MavenResolver resolver)
      throws Exception {
    if (!config.isWorkspace() && module != null) {
      throw new IllegalArgumentException("--module requires a workspace");
    }
    List<WorkspaceProject> projects =
        module == null && config.isWorkspace()
            ? workspace.ordered()
            : List.of(workspace.find(module == null ? config.name() : module));
    for (WorkspaceProject project : projects) {
      BuildEngine.FormatResult result = BuildEngine.format(project, resolver, check);
      if (check && !result.formatted()) {
        throw new IllegalStateException(
            "format check failed for " + project.name() + ":\n" + result.diagnostics());
      }
      System.out.println(
          check ? "fmt: " + project.name() + " is formatted" : "fmt: formatted " + project.name());
    }
  }

  private static Path findProjectRoot(Path start) {
    for (Path current = start; current != null; current = current.getParent()) {
      if (Files.isRegularFile(current.resolve("jot.toml"))) return current;
    }
    throw new IllegalArgumentException("could not find jot.toml from " + start);
  }
}
