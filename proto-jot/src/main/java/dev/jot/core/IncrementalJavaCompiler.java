package dev.jot.core;

import com.sun.source.tree.CompilationUnitTree;
import com.sun.source.util.JavacTask;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.Set;
import java.util.stream.Stream;
import javax.tools.JavaCompiler;
import javax.tools.StandardJavaFileManager;
import javax.tools.ToolProvider;

/** Compiles changed sources in place when their binary API remains compatible. */
final class IncrementalJavaCompiler {
  private static final String SOURCE_PREFIX = "compile-source.";
  private static final String SOURCE_SUFFIX = ".sha256";

  private IncrementalJavaCompiler() {}

  static Result compile(
      Path projectRoot,
      Path classes,
      String javaVersion,
      Path javaHome,
      String classpath,
      String processorPath,
      Map<String, String> processorOptions,
      Path generatedSources,
      String compileFingerprint,
      List<Path> sources,
      Properties state)
      throws Exception {
    Map<String, String> currentHashes = sourceHashes(sources);
    Map<String, String> previousHashes = previousSourceHashes(state);
    String previousEnvironment = state.getProperty("compile-fingerprint");
    boolean processing = !processorPath.isBlank();
    boolean reusable =
        Files.isDirectory(classes)
            && compileFingerprint.equals(previousEnvironment)
            && classFingerprint(classes).equals(state.getProperty("compiled-classes-sha256"));

    List<Path> dirty = new ArrayList<>();
    boolean removedSource = false;
    if (reusable && !previousHashes.isEmpty()) {
      Set<String> currentKeys = currentHashes.keySet();
      removedSource = !currentKeys.containsAll(previousHashes.keySet());
      if (!removedSource) {
        for (Path source : sources) {
          if (!currentHashes.get(sourceKey(source)).equals(previousHashes.get(sourceKey(source))))
            dirty.add(source);
        }
      }
    }

    boolean fullRebuild =
        !reusable || previousHashes.isEmpty() || removedSource || (processing && !dirty.isEmpty());
    boolean compiled = fullRebuild || !dirty.isEmpty();
    if (fullRebuild) {
      compileAll(
          projectRoot,
          classes,
          javaVersion,
          javaHome,
          classpath,
          processorPath,
          processorOptions,
          generatedSources,
          sources);
    } else if (!dirty.isEmpty()) {
      boolean applied =
          compileIncrementally(
              projectRoot, classes, javaVersion, javaHome, classpath, sources, dirty);
      if (!applied) {
        fullRebuild = true;
        compileAll(
            projectRoot,
            classes,
            javaVersion,
            javaHome,
            classpath,
            processorPath,
            processorOptions,
            generatedSources,
            sources);
      }
    }

    for (String key : state.stringPropertyNames()) {
      if (key.startsWith(SOURCE_PREFIX) && key.endsWith(SOURCE_SUFFIX)) state.remove(key);
    }
    currentHashes.forEach((source, hash) -> state.setProperty(sourceStateKey(source), hash));
    String classHash = classFingerprint(classes);
    state.setProperty("compile-fingerprint", compileFingerprint);
    state.setProperty("compiled-classes-sha256", classHash);
    if (compiled && !fullRebuild) {
      System.out.println("compile: incrementally compiled " + dirty.size() + " source(s)");
    }
    return new Result(compiled, fullRebuild, dirty.size(), classHash);
  }

  private static boolean compileIncrementally(
      Path projectRoot,
      Path classes,
      String javaVersion,
      Path javaHome,
      String classpath,
      List<Path> allSources,
      List<Path> dirtySources)
      throws Exception {
    Map<Path, String> identities = sourceIdentities(allSources);
    Set<String> allIdentities = new LinkedHashSet<>();
    for (Map.Entry<Path, String> item : identities.entrySet()) {
      if (!allIdentities.add(item.getValue())) return false;
    }
    Set<String> dirtyIdentities = new LinkedHashSet<>();
    for (Path source : dirtySources) dirtyIdentities.add(identities.get(source));

    Path staging = Files.createTempDirectory(classes.getParent(), ".jot-compile-");
    try {
      String incrementalClasspath = joinClasspath(classes.toString(), classpath);
      compileTo(
          projectRoot,
          staging,
          javaVersion,
          javaHome,
          incrementalClasspath,
          "",
          Map.of(),
          staging.resolve("generated-sources"),
          dirtySources);
      Map<String, List<Path>> previousOutputs = ownedClassFiles(classes, identities.values());
      Map<String, List<Path>> newOutputs = ownedClassFiles(staging, identities.values());

      for (String identity : dirtyIdentities) {
        List<Path> oldFiles = previousOutputs.getOrDefault(identity, List.of());
        List<Path> newFiles = newOutputs.getOrDefault(identity, List.of());
        if (!relativeFiles(classes, oldFiles).equals(relativeFiles(staging, newFiles))) {
          return false;
        }
        for (Path oldFile : oldFiles) {
          Path newFile = staging.resolve(classes.relativize(oldFile));
          if (!publicApi(javaHome, oldFile).equals(publicApi(javaHome, newFile))) return false;
        }
      }

      for (String identity : dirtyIdentities) {
        for (Path oldFile : previousOutputs.getOrDefault(identity, List.of())) {
          Files.deleteIfExists(oldFile);
        }
        for (Path newFile : newOutputs.getOrDefault(identity, List.of())) {
          Path destination = classes.resolve(staging.relativize(newFile));
          Files.createDirectories(destination.getParent());
          Files.copy(newFile, destination, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        }
      }
      return true;
    } finally {
      deleteTree(staging);
    }
  }

  private static Map<Path, String> sourceIdentities(List<Path> sources) throws Exception {
    Map<Path, String> result = new LinkedHashMap<>();
    JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
    if (compiler == null)
      throw new IllegalStateException("a full JDK is required to compile Java sources");
    try (StandardJavaFileManager files =
        compiler.getStandardFileManager(null, null, StandardCharsets.UTF_8)) {
      for (Path source : sources) {
        var file = files.getJavaFileObjects(source.toFile());
        JavacTask task =
            (JavacTask) compiler.getTask(null, files, null, List.of("-proc:none"), null, file);
        CompilationUnitTree unit = task.parse().iterator().next();
        String packagePath =
            unit.getPackageName() == null
                ? ""
                : unit.getPackageName().toString().replace('.', '/') + "/";
        result.put(source, packagePath + source.getFileName());
      }
    }
    return result;
  }

  private static Map<String, List<Path>> ownedClassFiles(
      Path output, Iterable<String> sourceIdentities) throws IOException {
    Set<String> knownSources = new LinkedHashSet<>();
    sourceIdentities.forEach(knownSources::add);
    Map<String, List<Path>> result = new LinkedHashMap<>();
    if (!Files.isDirectory(output)) return result;
    try (Stream<Path> files = Files.walk(output)) {
      for (Path file :
          files
              .filter(Files::isRegularFile)
              .filter(path -> path.toString().endsWith(".class"))
              .sorted()
              .toList()) {
        ClassFileInfo info = ClassFileInfo.read(file);
        if (info.sourceFile() == null) continue;
        int packageEnd = info.internalName().lastIndexOf('/');
        String packagePath = packageEnd < 0 ? "" : info.internalName().substring(0, packageEnd + 1);
        String identity = packagePath + info.sourceFile();
        if (knownSources.contains(identity)) {
          result.computeIfAbsent(identity, ignored -> new ArrayList<>()).add(file);
        }
      }
    }
    return result;
  }

  private static Set<Path> relativeFiles(Path root, List<Path> files) {
    Set<Path> relative = new LinkedHashSet<>();
    for (Path file : files) relative.add(root.relativize(file));
    return relative;
  }

  private static String publicApi(Path javaHome, Path classFile) throws Exception {
    Process process =
        new ProcessBuilder(
                jdkTool(javaHome, "javap"), "-p", "-s", "-constants", classFile.toString())
            .redirectErrorStream(true)
            .start();
    byte[] output = process.getInputStream().readAllBytes();
    int exit = process.waitFor();
    if (exit != 0)
      throw new IOException(
          "javap failed for " + classFile + ": " + new String(output, StandardCharsets.UTF_8));
    return new String(output, StandardCharsets.UTF_8);
  }

  private static void compileAll(
      Path projectRoot,
      Path classes,
      String javaVersion,
      Path javaHome,
      String classpath,
      String processorPath,
      Map<String, String> processorOptions,
      Path generatedSources,
      List<Path> sources)
      throws Exception {
    deleteTree(classes);
    Files.createDirectories(classes);
    deleteTree(generatedSources);
    if (!processorPath.isBlank()) Files.createDirectories(generatedSources);
    compileTo(
        projectRoot,
        classes,
        javaVersion,
        javaHome,
        classpath,
        processorPath,
        processorOptions,
        generatedSources,
        sources);
  }

  private static void compileTo(
      Path projectRoot,
      Path output,
      String javaVersion,
      Path javaHome,
      String classpath,
      String processorPath,
      Map<String, String> processorOptions,
      Path generatedSources,
      List<Path> sources)
      throws Exception {
    List<String> command =
        new ArrayList<>(
            List.of(jdkTool(javaHome, "javac"), "--release", javaVersion, "-d", output.toString()));
    if (!classpath.isBlank()) command.addAll(List.of("-classpath", classpath));
    if (processorPath.isBlank()) {
      command.add("-proc:none");
    } else {
      command.addAll(List.of("-processorpath", processorPath, "-s", generatedSources.toString()));
      processorOptions.forEach((key, value) -> command.add("-A" + key + "=" + value));
    }
    sources.forEach(path -> command.add(path.toString()));
    int status =
        new ProcessBuilder(command).directory(projectRoot.toFile()).inheritIO().start().waitFor();
    if (status != 0) {
      throw new IllegalStateException(
          "command failed with exit code " + status + ": " + String.join(" ", command));
    }
  }

  private static String joinClasspath(String first, String rest) {
    return rest.isBlank() ? first : first + System.getProperty("path.separator") + rest;
  }

  private static Map<String, String> sourceHashes(List<Path> sources) throws Exception {
    Map<String, String> hashes = new LinkedHashMap<>();
    for (Path source : sources) hashes.put(sourceKey(source), fileFingerprint(source));
    return hashes;
  }

  private static Map<String, String> previousSourceHashes(Properties state) {
    Map<String, String> hashes = new LinkedHashMap<>();
    for (String key : state.stringPropertyNames()) {
      if (!key.startsWith(SOURCE_PREFIX) || !key.endsWith(SOURCE_SUFFIX)) continue;
      hashes.put(decodeSourceKey(key), state.getProperty(key));
    }
    return hashes;
  }

  private static String sourceKey(Path source) {
    return source.toAbsolutePath().normalize().toString();
  }

  private static String sourceStateKey(String source) {
    String encoded =
        Base64.getUrlEncoder()
            .withoutPadding()
            .encodeToString(source.getBytes(StandardCharsets.UTF_8));
    return SOURCE_PREFIX + encoded + SOURCE_SUFFIX;
  }

  private static String decodeSourceKey(String key) {
    String encoded = key.substring(SOURCE_PREFIX.length(), key.length() - SOURCE_SUFFIX.length());
    return new String(Base64.getUrlDecoder().decode(encoded), StandardCharsets.UTF_8);
  }

  private static String fileFingerprint(Path file) throws Exception {
    return HexFormat.of()
        .formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(file)));
  }

  private static String classFingerprint(Path directory) throws Exception {
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    try (Stream<Path> paths = Files.walk(directory)) {
      for (Path file :
          paths
              .filter(Files::isRegularFile)
              .filter(path -> path.toString().endsWith(".class"))
              .sorted()
              .toList()) {
        digest.update(directory.relativize(file).toString().getBytes(StandardCharsets.UTF_8));
        digest.update((byte) 0);
        digest.update(Files.readAllBytes(file));
      }
    }
    return HexFormat.of().formatHex(digest.digest());
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

  record Result(boolean compiled, boolean fullRebuild, int dirtySources, String classHash) {}
}
