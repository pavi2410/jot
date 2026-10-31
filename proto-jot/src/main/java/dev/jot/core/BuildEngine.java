package dev.jot.core;

import dev.jot.core.resolve.Lockfile;
import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.List;
import java.util.Properties;
import java.util.stream.Stream;

/** Compiles configured Java sources, packages JARs, and launches project mains. */
public final class BuildEngine {
  private BuildEngine() {}

  public static void build(WorkspaceProject project) throws Exception {
    build(project, new MavenResolver());
  }

  public static void build(WorkspaceProject project, MavenResolver resolver) throws Exception {
    Path root = project.root();
    ProjectConfig config = project.config();
    Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
    List<MavenResolver.ResolvedArtifact> externalArtifacts =
        resolveExternalArtifacts(project, resolver);
    List<MavenResolver.ResolvedArtifact> processorArtifacts =
        resolveProcessorArtifacts(project, resolver);
    List<Path> sources = new ArrayList<>();
    for (String sourceDirectory : config.sourceDirectories()) {
      Path sourceRoot = root.resolve(sourceDirectory).normalize();
      if (!sourceRoot.startsWith(root)) {
        throw new IllegalArgumentException("source directory escapes project: " + sourceDirectory);
      }
      if (!Files.isDirectory(sourceRoot)) continue;
      try (Stream<Path> paths = Files.walk(sourceRoot)) {
        sources.addAll(paths.filter(path -> path.toString().endsWith(".java")).toList());
      }
    }
    sources.sort(Comparator.naturalOrder());
    if (sources.isEmpty()) {
      throw new IllegalStateException(
          "no Java sources in configured build.source-dirs: " + config.sourceDirectories());
    }
    List<ResourceFile> resources = collectResources(root, config.resourceDirectories());

    Path classes = root.resolve(config.outputDirectory()).normalize();
    Path target = root.resolve("target");
    if (!classes.startsWith(target) || classes.equals(target)) {
      throw new IllegalArgumentException("build.output-directory must be below target/");
    }
    Path artifact = target.resolve(config.name() + "-" + config.version() + ".jar");
    Path displayArtifact =
        root.toAbsolutePath().normalize().relativize(artifact.toAbsolutePath().normalize());
    Path stateFile = target.resolve(".jot/build-state.properties");
    String compileFingerprint =
        compileFingerprint(
            project, externalArtifacts, processorArtifacts, config.processorOptions(), javaHome);
    Properties previousState = readState(stateFile);
    IncrementalJavaCompiler.Result compilation =
        IncrementalJavaCompiler.compile(
            root,
            classes,
            config.javaVersion(),
            javaHome,
            classpath(project, externalArtifacts),
            processorArtifacts.stream()
                .map(processor -> processor.jar().toString())
                .collect(java.util.stream.Collectors.joining(System.getProperty("path.separator"))),
            config.processorOptions(),
            target.resolve("generated-sources"),
            compileFingerprint,
            sources,
            previousState);
    boolean compileRequired = compilation.compiled();

    copyResources(resources, classes, previousState);
    String classesHash = compilation.classHash();
    String outputHash = directoryFingerprint(classes);
    String packageFingerprint = packageFingerprint(config, classesHash);
    boolean artifactIsIntact =
        Files.isRegularFile(artifact)
            && packageFingerprint.equals(previousState.getProperty("package-fingerprint"))
            && outputHash.equals(previousState.getProperty("output-tree-sha256"))
            && fileFingerprint(artifact).equals(previousState.getProperty("artifact-sha256"));
    if (!artifactIsIntact) {
      Files.createDirectories(artifact.getParent());
      List<String> jar =
          new ArrayList<>(
              List.of(
                  jdkTool(javaHome, "jar"),
                  "--create",
                  "--file",
                  artifact.toString(),
                  "--date=1980-01-01T00:00:02Z"));
      if (!config.mainClass().isBlank()) {
        jar.addAll(List.of("--main-class", config.mainClass()));
      }
      jar.addAll(List.of("-C", classes.toString(), "."));
      runProcess(root, jar);
    }
    Properties state = new Properties();
    for (String key : previousState.stringPropertyNames()) {
      if (key.startsWith("resource.") || key.startsWith("compile-source.")) {
        state.setProperty(key, previousState.getProperty(key));
      }
    }
    state.setProperty("compile-fingerprint", compileFingerprint);
    state.setProperty("compiled-classes-sha256", classesHash);
    state.setProperty("package-fingerprint", packageFingerprint);
    state.setProperty("output-tree-sha256", outputHash);
    state.setProperty("artifact-sha256", fileFingerprint(artifact));
    Files.createDirectories(stateFile.getParent());
    try (var output = Files.newOutputStream(stateFile)) {
      state.store(output, "Jot content fingerprint. Generated file.");
    }
    if (compileRequired) {
      System.out.println(
          "build: " + config.name() + " " + config.version() + " -> " + displayArtifact);
    } else if (artifactIsIntact) {
      System.out.println("build: " + config.name() + " is up to date");
    } else {
      System.out.println("package: " + config.name() + " -> " + displayArtifact);
    }
  }

  public static void run(WorkspaceProject project, List<String> arguments) throws Exception {
    run(project, arguments, new MavenResolver());
  }

  public static void run(WorkspaceProject project, List<String> arguments, MavenResolver resolver)
      throws Exception {
    Path javaHome =
        JavaToolchain.javaHome(project.config().javaVersion(), project.config().javaVendor());
    List<String> classpathEntries = new ArrayList<>();
    classpathEntries.add(project.root().resolve(project.config().outputDirectory()).toString());
    for (WorkspaceProject dependency : transitiveDependencies(project)) {
      classpathEntries.add(
          dependency.root().resolve(dependency.config().outputDirectory()).toString());
    }
    for (MavenResolver.ResolvedArtifact artifact : resolveExternalArtifacts(project, resolver)) {
      classpathEntries.add(artifact.jar().toString());
    }
    List<String> command =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "java"),
                "-cp",
                String.join(System.getProperty("path.separator"), classpathEntries),
                project.config().mainClass()));
    command.addAll(arguments);
    runProcess(project.root(), command);
  }

  public static Path doc(WorkspaceProject project, MavenResolver resolver) throws Exception {
    Path root = project.root();
    ProjectConfig config = project.config();
    Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
    List<Path> sources = collectSources(root, config.sourceDirectories());
    if (sources.isEmpty()) {
      throw new IllegalStateException(
          "no Java sources in configured build.source-dirs: " + config.sourceDirectories());
    }
    List<MavenResolver.ResolvedArtifact> dependencies = resolveExternalArtifacts(project, resolver);
    Path docs = root.resolve("target/docs");
    deleteTree(docs);
    Files.createDirectories(docs);
    List<String> command =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "javadoc"),
                "--release",
                config.javaVersion(),
                "-quiet",
                "-Xdoclint:all,-missing",
                "-d",
                docs.toString()));
    String classpath = classpath(project, dependencies);
    if (!classpath.isBlank()) command.addAll(List.of("-classpath", classpath));
    command.addAll(sources.stream().map(Path::toString).toList());
    runProcess(root, command);
    return docs;
  }

  public static LintResult lint(WorkspaceProject project, MavenResolver resolver) throws Exception {
    Path root = project.root();
    ProjectConfig config = project.config();
    Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
    List<Path> sources = collectSources(root, config.sourceDirectories());
    if (sources.isEmpty()) {
      throw new IllegalStateException(
          "no Java sources in configured build.source-dirs: " + config.sourceDirectories());
    }
    List<MavenResolver.ResolvedArtifact> dependencies = resolveExternalArtifacts(project, resolver);
    Path target = root.resolve("target");
    Files.createDirectories(target);
    Path lintClasses = Files.createTempDirectory(target, ".jot-lint-");
    List<String> command =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "javac"),
                "--release",
                config.javaVersion(),
                "-Xlint:all",
                "-proc:none",
                "-d",
                lintClasses.toString()));
    String classpath = classpath(project, dependencies);
    if (!classpath.isBlank()) command.addAll(List.of("-classpath", classpath));
    command.addAll(sources.stream().map(Path::toString).toList());
    try {
      Process process =
          new ProcessBuilder(command).directory(root.toFile()).redirectErrorStream(true).start();
      String diagnostics = new String(process.getInputStream().readAllBytes());
      int status = process.waitFor();
      if (status != 0) {
        throw new IllegalStateException(
            "javac lint failed with exit code " + status + ":\n" + diagnostics);
      }
      return new LintResult(diagnostics.isBlank(), diagnostics);
    } finally {
      deleteTree(lintClasses);
    }
  }

  public record LintResult(boolean clean, String diagnostics) {}

  public static FormatResult format(WorkspaceProject project, MavenResolver resolver, boolean check)
      throws Exception {
    Path root = project.root();
    ProjectConfig config = project.config();
    Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
    if (config.javaFormatter() == null || config.javaFormatter().isBlank()) {
      throw new IllegalArgumentException(
          "missing [format].java in jot.toml; set it to a pinned formatter coordinate");
    }
    List<Path> sources = new ArrayList<>(collectSources(root, config.sourceDirectories()));
    sources.addAll(collectSources(root, config.testSourceDirectories()));
    sources = sources.stream().distinct().sorted().toList();
    if (sources.isEmpty()) return new FormatResult(true, "");

    MavenCoordinate formatter = MavenCoordinate.parse(config.javaFormatter());
    List<MavenResolver.ResolvedArtifact> artifacts =
        resolver.resolve(java.util.Map.of("formatter", formatter.notation()));
    Path lockPath = findLockfile(root);
    if (lockPath != null) Lockfile.read(lockPath).verify(List.of(formatter.notation()), artifacts);

    List<String> command =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "java"),
                "--add-exports=jdk.compiler/com.sun.tools.javac.api=ALL-UNNAMED",
                "--add-exports=jdk.compiler/com.sun.tools.javac.code=ALL-UNNAMED",
                "--add-exports=jdk.compiler/com.sun.tools.javac.file=ALL-UNNAMED",
                "--add-exports=jdk.compiler/com.sun.tools.javac.parser=ALL-UNNAMED",
                "--add-exports=jdk.compiler/com.sun.tools.javac.tree=ALL-UNNAMED",
                "--add-exports=jdk.compiler/com.sun.tools.javac.util=ALL-UNNAMED",
                "-classpath",
                String.join(
                    System.getProperty("path.separator"),
                    artifacts.stream().map(artifact -> artifact.jar().toString()).toList()),
                "com.google.googlejavaformat.java.Main"));
    if (config.javaFormatterStyle().equals("aosp")) command.add("--aosp");
    if (check) command.addAll(List.of("--dry-run", "--set-exit-if-changed"));
    else command.add("--replace");
    command.addAll(sources.stream().map(Path::toString).toList());
    Process process =
        new ProcessBuilder(command).directory(root.toFile()).redirectErrorStream(true).start();
    String output = new String(process.getInputStream().readAllBytes());
    int status = process.waitFor();
    if (check && status != 0 && output.contains("error:")) {
      throw new IllegalStateException("Java formatter failed:\n" + output);
    }
    if (!check && status != 0) {
      throw new IllegalStateException(
          "Java formatter failed with exit code " + status + ":\n" + output);
    }
    return new FormatResult(status == 0, output);
  }

  public record FormatResult(boolean formatted, String diagnostics) {}

  public static void clean(WorkspaceProject project) throws IOException {
    clean(project, null);
  }

  public static void clean(WorkspaceProject project, Path preserveArtifact) throws IOException {
    Path target = project.root().resolve("target").toAbsolutePath().normalize();
    Path savedArtifact = null;
    if (preserveArtifact != null) {
      Path artifact = preserveArtifact.toAbsolutePath().normalize();
      if (!artifact.startsWith(target) || !Files.isRegularFile(artifact)) {
        throw new IllegalArgumentException(
            "preserved artifact must be a file inside the project target directory");
      }
      savedArtifact = Files.createTempFile("jot-clean-bootstrap-", ".jar");
      Files.copy(artifact, savedArtifact, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
    }
    try {
      deleteTree(target);
      if (savedArtifact != null) {
        Files.createDirectories(preserveArtifact.toAbsolutePath().normalize().getParent());
        Files.copy(
            savedArtifact,
            preserveArtifact.toAbsolutePath().normalize(),
            java.nio.file.StandardCopyOption.REPLACE_EXISTING);
      }
    } finally {
      if (savedArtifact != null) Files.deleteIfExists(savedArtifact);
    }
    System.out.println("clean: " + project.config().name());
  }

  public static void lock(ProjectWorkspace workspace, Path lockPath, MavenResolver resolver)
      throws Exception {
    lock(workspace, lockPath, resolver, Integer.MAX_VALUE);
  }

  public static void lock(
      ProjectWorkspace workspace, Path lockPath, MavenResolver resolver, int maxDepth)
      throws Exception {
    java.util.Map<String, String> declarations = new java.util.LinkedHashMap<>();
    int index = 0;
    for (WorkspaceProject project : workspace.ordered()) {
      for (String coordinate : project.config().dependencies().values()) {
        declarations.put("dependency-" + index++, coordinate);
      }
      for (String coordinate : project.config().testDependencies().values()) {
        declarations.put("test-dependency-" + index++, coordinate);
      }
      for (String coordinate : project.config().processors().values()) {
        declarations.put("processor-" + index++, coordinate);
      }
      if (project.config().javaFormatter() != null && !project.config().javaFormatter().isBlank()) {
        declarations.putIfAbsent(
            "java-formatter-" + project.config().javaFormatter(), project.config().javaFormatter());
      }
      if (!collectSources(project.root(), project.config().benchmarks().sourceDirectories())
          .isEmpty()) {
        String jmhVersion = project.config().benchmarks().jmhVersion();
        declarations.put("jmh-core-" + project.name(), "org.openjdk.jmh:jmh-core:" + jmhVersion);
        declarations.put(
            "jmh-generator-annprocess-" + project.name(),
            "org.openjdk.jmh:jmh-generator-annprocess:" + jmhVersion);
        for (int benchIndex = 0;
            benchIndex < project.config().benchmarks().dependencies().size();
            benchIndex++) {
          declarations.put(
              "bench-" + project.name() + "-" + benchIndex,
              project.config().benchmarks().dependencies().get(benchIndex));
        }
      }
    }
    List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(declarations, maxDepth);
    Lockfile.create(declarations.values(), resolved).write(lockPath);
    System.out.println("lock: wrote " + lockPath);
  }

  public static void test(WorkspaceProject project, MavenResolver resolver) throws Exception {
    build(project, resolver);
    Path root = project.root();
    ProjectConfig config = project.config();
    Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
    List<Path> testSources = collectSources(root, config.testSourceDirectories());
    if (testSources.isEmpty()) {
      System.out.println("test: no test sources in " + config.name());
      return;
    }
    Path testClasses = root.resolve(config.testOutputDirectory()).normalize();
    Path target = root.resolve("target");
    if (!testClasses.startsWith(target) || testClasses.equals(target)) {
      throw new IllegalArgumentException("test.output-directory must be below target/");
    }
    deleteTree(testClasses);
    Files.createDirectories(testClasses);

    List<MavenResolver.ResolvedArtifact> mainArtifacts =
        resolveExternalArtifacts(project, resolver);
    List<MavenResolver.ResolvedArtifact> testArtifacts =
        resolver.resolve(config.testDependencies());
    Path lockPath = findLockfile(root);
    if (lockPath != null)
      Lockfile.read(lockPath).verify(config.testDependencies().values(), testArtifacts);
    List<String> classpathEntries = new ArrayList<>();
    classpathEntries.add(root.resolve(config.outputDirectory()).toString());
    for (WorkspaceProject dependency : transitiveDependencies(project)) {
      classpathEntries.add(
          dependency.root().resolve(dependency.config().outputDirectory()).toString());
    }
    mainArtifacts.stream()
        .map(artifact -> artifact.jar().toString())
        .forEach(classpathEntries::add);
    testArtifacts.stream()
        .map(artifact -> artifact.jar().toString())
        .forEach(classpathEntries::add);
    List<String> testClasspath = List.copyOf(classpathEntries);

    List<String> javac =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "javac"),
                "--release",
                config.javaVersion(),
                "-classpath",
                String.join(System.getProperty("path.separator"), testClasspath),
                "-d",
                testClasses.toString()));
    testSources.forEach(path -> javac.add(path.toString()));
    runProcess(root, javac);

    List<String> runtimeClasspath = new ArrayList<>();
    runtimeClasspath.add(testClasses.toString());
    runtimeClasspath.addAll(testClasspath);
    List<String> runner =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "java"),
                "-cp",
                String.join(System.getProperty("path.separator"), runtimeClasspath),
                "--enable-final-field-mutation=ALL-UNNAMED",
                "org.junit.platform.console.ConsoleLauncher",
                "execute",
                "--scan-class-path",
                "--fail-if-no-tests",
                "--disable-ansi-colors"));
    runProcess(root, runner);
  }

  public static boolean bench(
      WorkspaceProject project,
      MavenResolver resolver,
      String filter,
      int forks,
      Integer warmup,
      Integer iterations)
      throws Exception {
    build(project, resolver);
    Path root = project.root();
    ProjectConfig config = project.config();
    List<Path> sources = collectSources(root, config.benchmarks().sourceDirectories());
    if (sources.isEmpty()) {
      System.out.println("bench: no benchmark sources found for " + config.name());
      return false;
    }

    java.util.Map<String, String> benchmarkCoordinates = new java.util.LinkedHashMap<>();
    java.util.Map<String, String> allCoordinates = new java.util.LinkedHashMap<>();
    List<WorkspaceProject> dependencyProjects = new ArrayList<>(transitiveDependencies(project));
    dependencyProjects.add(project);
    int dependencyIndex = 0;
    for (WorkspaceProject dependencyProject : dependencyProjects) {
      for (String coordinate : dependencyProject.config().dependencies().values()) {
        allCoordinates.put("dependency-" + dependencyIndex++, coordinate);
      }
    }
    benchmarkCoordinates.put(
        "jmh-core", "org.openjdk.jmh:jmh-core:" + config.benchmarks().jmhVersion());
    benchmarkCoordinates.put(
        "jmh-generator-annprocess",
        "org.openjdk.jmh:jmh-generator-annprocess:" + config.benchmarks().jmhVersion());
    for (int index = 0; index < config.benchmarks().dependencies().size(); index++) {
      benchmarkCoordinates.put("benchmark-" + index, config.benchmarks().dependencies().get(index));
    }
    allCoordinates.putAll(benchmarkCoordinates);
    List<MavenResolver.ResolvedArtifact> benchmarkDependencies = resolver.resolve(allCoordinates);
    Path lockPath = findLockfile(root);
    if (lockPath != null) {
      Lockfile.read(lockPath).verify(allCoordinates.values(), benchmarkDependencies);
    }
    if (benchmarkDependencies.stream()
        .noneMatch(
            artifact -> artifact.coordinate().artifact().equals("jmh-generator-annprocess"))) {
      throw new IllegalStateException("JMH annotation processor was not resolved");
    }

    Path target = root.resolve("target");
    Path classes = target.resolve("bench-classes");
    Path generated = target.resolve("generated-bench-sources");
    deleteTree(classes);
    deleteTree(generated);
    Files.createDirectories(classes);
    Files.createDirectories(generated);
    List<String> classpathEntries = new ArrayList<>();
    classpathEntries.add(root.resolve(config.outputDirectory()).toString());
    String mainClasspath = classpath(project, benchmarkDependencies);
    if (!mainClasspath.isBlank()) classpathEntries.add(mainClasspath);
    String joinedClasspath = String.join(System.getProperty("path.separator"), classpathEntries);
    Path javaHome = JavaToolchain.javaHome(config.javaVersion(), config.javaVendor());
    List<String> compile =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "javac"),
                "--release",
                config.javaVersion(),
                "-classpath",
                joinedClasspath,
                "-processorpath",
                String.join(
                    System.getProperty("path.separator"),
                    benchmarkDependencies.stream().map(item -> item.jar().toString()).toList()),
                "-d",
                classes.toString(),
                "-s",
                generated.toString()));
    sources.forEach(source -> compile.add(source.toString()));
    runProcess(root, compile);

    List<String> runtimeClasspath = new ArrayList<>();
    runtimeClasspath.add(classes.toString());
    runtimeClasspath.addAll(classpathEntries);
    List<String> run =
        new ArrayList<>(
            List.of(
                jdkTool(javaHome, "java"),
                "-cp",
                String.join(System.getProperty("path.separator"), runtimeClasspath),
                "org.openjdk.jmh.Main"));
    if (filter != null) run.add(filter);
    run.addAll(List.of("-f", Integer.toString(forks)));
    if (warmup != null) run.addAll(List.of("-wi", warmup.toString()));
    if (iterations != null) run.addAll(List.of("-i", iterations.toString()));
    runProcess(root, run);
    return true;
  }

  private static String compileFingerprint(
      WorkspaceProject project,
      List<MavenResolver.ResolvedArtifact> externalArtifacts,
      List<MavenResolver.ResolvedArtifact> processorArtifacts,
      java.util.Map<String, String> processorOptions,
      Path javaHome)
      throws IOException, NoSuchAlgorithmException {
    Path root = project.root();
    ProjectConfig config = project.config();
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    updateDigest(digest, config.javaVersion());
    updateDigest(digest, java.util.Objects.toString(config.javaVendor(), ""));
    updateDigest(digest, config.sourceDirectories().toString());
    updateDigest(digest, config.outputDirectory());
    updateDigest(digest, javaHome.toString());
    for (WorkspaceProject dependency : transitiveDependencies(project)) {
      updateDigest(digest, dependency.root().toString());
      Path dependencyClasses = dependency.root().resolve(dependency.config().outputDirectory());
      if (Files.isDirectory(dependencyClasses)) {
        try (Stream<Path> files = Files.walk(dependencyClasses)) {
          for (Path file :
              files
                  .filter(Files::isRegularFile)
                  .filter(path -> path.toString().endsWith(".class"))
                  .sorted()
                  .toList()) {
            updateDigest(digest, dependencyClasses.relativize(file).toString());
            digest.update(Files.readAllBytes(file));
          }
        }
      }
    }
    for (MavenResolver.ResolvedArtifact artifact : externalArtifacts) {
      updateDigest(digest, artifact.coordinate().notation());
      digest.update(Files.readAllBytes(artifact.jar()));
    }
    for (MavenResolver.ResolvedArtifact artifact : processorArtifacts) {
      updateDigest(digest, artifact.coordinate().notation());
      digest.update(Files.readAllBytes(artifact.jar()));
    }
    processorOptions.forEach(
        (key, value) -> {
          updateDigest(digest, key);
          updateDigest(digest, value);
        });
    return HexFormat.of().formatHex(digest.digest());
  }

  private static String packageFingerprint(ProjectConfig config, String classesHash)
      throws NoSuchAlgorithmException {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    updateDigest(digest, config.name());
    updateDigest(digest, config.version());
    updateDigest(digest, config.mainClass());
    updateDigest(digest, classesHash);
    return HexFormat.of().formatHex(digest.digest());
  }

  private static void updateDigest(MessageDigest digest, String value) {
    digest.update(value.getBytes(StandardCharsets.UTF_8));
    digest.update((byte) 0);
  }

  private static Properties readState(Path stateFile) throws IOException {
    Properties state = new Properties();
    if (Files.isRegularFile(stateFile)) {
      try (var input = Files.newInputStream(stateFile)) {
        state.load(input);
      }
    }
    return state;
  }

  private static String directoryFingerprint(Path directory) throws Exception {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    try (Stream<Path> paths = Files.walk(directory)) {
      for (Path file : paths.filter(Files::isRegularFile).sorted().toList()) {
        updateDigest(digest, directory.relativize(file).toString());
        digest.update(Files.readAllBytes(file));
      }
    }
    return HexFormat.of().formatHex(digest.digest());
  }

  private static String fileFingerprint(Path file) throws Exception {
    return HexFormat.of()
        .formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(file)));
  }

  private static String classpath(
      WorkspaceProject project, List<MavenResolver.ResolvedArtifact> externalArtifacts) {
    List<String> entries =
        new ArrayList<>(
            transitiveDependencies(project).stream()
                .map(
                    dependency ->
                        dependency.root().resolve(dependency.config().outputDirectory()).toString())
                .toList());
    externalArtifacts.stream().map(artifact -> artifact.jar().toString()).forEach(entries::add);
    return String.join(System.getProperty("path.separator"), entries);
  }

  private static List<Path> collectSources(Path root, List<String> sourceDirectories)
      throws IOException {
    List<Path> sources = new ArrayList<>();
    for (String sourceDirectory : sourceDirectories) {
      Path sourceRoot = root.resolve(sourceDirectory).normalize();
      if (!sourceRoot.startsWith(root)) {
        throw new IllegalArgumentException("source directory escapes project: " + sourceDirectory);
      }
      if (!Files.isDirectory(sourceRoot)) continue;
      try (Stream<Path> paths = Files.walk(sourceRoot)) {
        sources.addAll(paths.filter(path -> path.toString().endsWith(".java")).toList());
      }
    }
    sources.sort(Comparator.naturalOrder());
    return sources;
  }

  private static List<ResourceFile> collectResources(Path root, List<String> resourceDirectories)
      throws IOException {
    java.util.Map<Path, ResourceFile> resources = new java.util.LinkedHashMap<>();
    for (String directory : resourceDirectories) {
      Path resourceRoot = root.resolve(directory).normalize();
      if (!resourceRoot.startsWith(root)) {
        throw new IllegalArgumentException("resource directory escapes project: " + directory);
      }
      if (!Files.isDirectory(resourceRoot)) continue;
      try (Stream<Path> paths = Files.walk(resourceRoot)) {
        for (Path file : paths.filter(Files::isRegularFile).toList()) {
          Path relativePath = resourceRoot.relativize(file);
          ResourceFile previous =
              resources.putIfAbsent(relativePath, new ResourceFile(file, relativePath));
          if (previous != null && !previous.source().equals(file)) {
            throw new IllegalArgumentException(
                "resource path is declared more than once: " + relativePath);
          }
        }
      }
    }
    return resources.values().stream()
        .sorted(Comparator.comparing(resource -> resource.relativePath().toString()))
        .toList();
  }

  private static void copyResources(
      List<ResourceFile> resources, Path classes, Properties previousState) throws Exception {
    java.util.Set<String> expectedKeys = new java.util.LinkedHashSet<>();
    for (ResourceFile resource : resources)
      expectedKeys.add(resourceStateKey(resource.relativePath()));
    List<String> oldResourceKeys =
        previousState.stringPropertyNames().stream()
            .filter(key -> key.startsWith("resource.") && key.endsWith(".sha256"))
            .toList();
    for (String key : oldResourceKeys) {
      if (expectedKeys.contains(key)) continue;
      Path stale = classes.resolve(decodeResourcePath(key)).normalize();
      if (!stale.startsWith(classes)) {
        throw new IllegalStateException("invalid resource path in build state: " + key);
      }
      if (Files.deleteIfExists(stale)) removeEmptyDirectories(stale.getParent(), classes);
      previousState.remove(key);
    }
    for (ResourceFile resource : resources) {
      Path destination = classes.resolve(resource.relativePath()).normalize();
      if (!destination.startsWith(classes)) {
        throw new IllegalArgumentException(
            "resource path escapes build output: " + resource.relativePath());
      }
      String key = resourceStateKey(resource.relativePath());
      String expectedHash = fileFingerprint(resource.source());
      if (expectedHash.equals(previousState.getProperty(key))
          && Files.isRegularFile(destination)
          && expectedHash.equals(fileFingerprint(destination))) continue;
      Files.createDirectories(destination.getParent());
      Files.copy(resource.source(), destination, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
      previousState.setProperty(key, expectedHash);
    }
  }

  private static String resourceStateKey(Path relativePath) {
    String encoded =
        java.util.Base64.getUrlEncoder()
            .withoutPadding()
            .encodeToString(relativePath.toString().getBytes(StandardCharsets.UTF_8));
    return "resource." + encoded + ".sha256";
  }

  private static Path decodeResourcePath(String key) {
    String encoded = key.substring("resource.".length(), key.length() - ".sha256".length());
    return Path.of(
        new String(java.util.Base64.getUrlDecoder().decode(encoded), StandardCharsets.UTF_8));
  }

  private static void removeEmptyDirectories(Path directory, Path stop) throws IOException {
    for (Path current = directory;
        current != null && !current.equals(stop) && current.startsWith(stop);
        current = current.getParent()) {
      try (Stream<Path> children = Files.list(current)) {
        if (children.findAny().isPresent()) return;
      }
      Files.delete(current);
    }
  }

  private static List<MavenResolver.ResolvedArtifact> resolveExternalArtifacts(
      WorkspaceProject project, MavenResolver resolver) throws Exception {
    List<WorkspaceProject> dependencyProjects = new ArrayList<>();
    dependencyProjects.add(project);
    dependencyProjects.addAll(transitiveDependencies(project));
    java.util.Map<String, String> declarations = new java.util.LinkedHashMap<>();
    int index = 0;
    for (WorkspaceProject dependencyProject : dependencyProjects) {
      for (String coordinate : dependencyProject.config().dependencies().values()) {
        declarations.put("dependency-" + index++, coordinate);
      }
    }
    List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(declarations);
    Path lockPath = findLockfile(project.root());
    if (lockPath != null) Lockfile.read(lockPath).verify(declarations.values(), resolved);
    return resolved;
  }

  private static List<MavenResolver.ResolvedArtifact> resolveProcessorArtifacts(
      WorkspaceProject project, MavenResolver resolver) throws Exception {
    List<MavenResolver.ResolvedArtifact> resolved = resolver.resolve(project.config().processors());
    Path lockPath = findLockfile(project.root());
    if (lockPath != null)
      Lockfile.read(lockPath).verify(project.config().processors().values(), resolved);
    return resolved;
  }

  private static Path findLockfile(Path projectRoot) {
    for (Path current = projectRoot; current != null; current = current.getParent()) {
      Path candidate = current.resolve("jot.lock");
      if (Files.isRegularFile(candidate)) return candidate;
    }
    return null;
  }

  private static List<WorkspaceProject> transitiveDependencies(WorkspaceProject project) {
    List<WorkspaceProject> result = new ArrayList<>();
    java.util.Set<Path> seen = new java.util.LinkedHashSet<>();
    for (WorkspaceProject dependency : project.dependencies()) {
      appendDependency(dependency, seen, result);
    }
    return result;
  }

  private static void appendDependency(
      WorkspaceProject project, java.util.Set<Path> seen, List<WorkspaceProject> result) {
    if (!seen.add(project.root())) return;
    result.add(project);
    for (WorkspaceProject dependency : project.dependencies()) {
      appendDependency(dependency, seen, result);
    }
  }

  private static void runProcess(Path directory, List<String> command)
      throws IOException, InterruptedException {
    int status =
        new ProcessBuilder(command).directory(directory.toFile()).inheritIO().start().waitFor();
    if (status != 0) {
      throw new IllegalStateException(
          "command failed with exit code " + status + ": " + String.join(" ", command));
    }
  }

  private static String jdkTool(Path javaHome, String name) {
    return JavaToolchain.tool(javaHome, name).toString();
  }

  private static void deleteTree(Path root) throws IOException {
    if (!Files.exists(root)) return;
    try (Stream<Path> paths = Files.walk(root)) {
      for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
    }
  }

  private record ResourceFile(Path source, Path relativePath) {}
}
