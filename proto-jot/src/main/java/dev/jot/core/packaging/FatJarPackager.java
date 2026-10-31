package dev.jot.core.packaging;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.Enumeration;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

/** Combines runtime JARs and project classes into one executable JAR. */
public final class FatJarPackager {
  private FatJarPackager() {}

  public static List<String> packageJar(
      Path projectRoot,
      Path targetDirectory,
      Path output,
      Path jarTool,
      String mainClass,
      List<Path> dependencyJars,
      List<Path> dependencyDirectories,
      Path classes)
      throws IOException, InterruptedException {
    Path staging = targetDirectory.resolve(".jot/fat-jar-staging");
    deleteTree(staging);
    Files.createDirectories(staging);
    Map<String, LinkedHashSet<String>> services = new TreeMap<>();
    List<String> warnings = new ArrayList<>();
    try {
      for (Path dependencyJar : dependencyJars) {
        if (Files.isRegularFile(dependencyJar)) {
          overlayJar(dependencyJar, staging, services, warnings);
        }
      }
      for (Path dependencyDirectory : dependencyDirectories) {
        overlayDirectory(dependencyDirectory, staging, services);
      }
      overlayDirectory(classes, staging, services);
      writeServices(staging, services);

      Files.createDirectories(output.getParent());
      Path temporaryOutput = Files.createTempFile(output.getParent(), ".jot-fat-", ".jar");
      try {
        Files.delete(temporaryOutput);
        List<String> command =
            List.of(
                jarTool.toString(),
                "--create",
                "--file",
                temporaryOutput.toString(),
                "--date=1980-01-01T00:00:02Z",
                "--main-class",
                mainClass,
                "-C",
                staging.toString(),
                ".");
        Process process =
            new ProcessBuilder(command)
                .directory(projectRoot.toFile())
                .redirectErrorStream(true)
                .start();
        String diagnostics =
            new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
        int status = process.waitFor();
        if (status != 0) {
          throw new IOException(
              "jar failed with exit code "
                  + status
                  + (diagnostics.isBlank() ? "" : ":\n" + diagnostics));
        }
        try {
          Files.move(
              temporaryOutput,
              output,
              StandardCopyOption.REPLACE_EXISTING,
              StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignored) {
          Files.move(temporaryOutput, output, StandardCopyOption.REPLACE_EXISTING);
        }
      } finally {
        Files.deleteIfExists(temporaryOutput);
      }
    } finally {
      deleteTree(staging);
    }
    return List.copyOf(warnings);
  }

  private static void overlayJar(
      Path jarPath,
      Path staging,
      Map<String, LinkedHashSet<String>> services,
      List<String> warnings)
      throws IOException {
    try (ZipFile jar = new ZipFile(jarPath.toFile())) {
      Enumeration<? extends ZipEntry> entries = jar.entries();
      while (entries.hasMoreElements()) {
        ZipEntry entry = entries.nextElement();
        if (entry.isDirectory()) continue;
        String name = safeEntryName(entry.getName());
        if (name == null || shouldSkip(name)) continue;
        if (isService(name)) {
          try (var input = jar.getInputStream(entry)) {
            mergeService(services, name, input.readAllBytes());
          }
          continue;
        }
        Path destination = staging.resolve(name).normalize();
        if (!destination.startsWith(staging)) continue;
        if (Files.exists(destination)) {
          boolean differs = false;
          if (name.endsWith(".class")) {
            try (var input = jar.getInputStream(entry)) {
              differs =
                  !java.util.Arrays.equals(Files.readAllBytes(destination), input.readAllBytes());
            }
          }
          if (differs) {
            warnings.add("duplicate class `" + name + "` (keeping first occurrence)");
          }
          continue;
        }
        Files.createDirectories(destination.getParent());
        try (var input = jar.getInputStream(entry)) {
          Files.copy(input, destination);
        }
      }
    }
  }

  private static void overlayDirectory(
      Path source, Path staging, Map<String, LinkedHashSet<String>> services) throws IOException {
    if (!Files.isDirectory(source)) return;
    try (var paths = Files.walk(source)) {
      for (Path file : paths.filter(Files::isRegularFile).sorted().toList()) {
        String name = safeEntryName(source.relativize(file).toString());
        if (name == null || shouldSkip(name)) continue;
        byte[] bytes = Files.readAllBytes(file);
        if (isService(name)) {
          mergeService(services, name, bytes);
          continue;
        }
        Path destination = staging.resolve(name).normalize();
        if (!destination.startsWith(staging)) continue;
        Files.createDirectories(destination.getParent());
        Files.write(destination, bytes);
      }
    }
  }

  private static void mergeService(
      Map<String, LinkedHashSet<String>> services, String name, byte[] contents) {
    LinkedHashSet<String> providers =
        services.computeIfAbsent(name, ignored -> new LinkedHashSet<>());
    for (String line : new String(contents, StandardCharsets.UTF_8).split("\\R")) {
      String provider = line.strip();
      if (!provider.isEmpty() && !provider.startsWith("#")) providers.add(provider);
    }
  }

  private static void writeServices(Path staging, Map<String, LinkedHashSet<String>> services)
      throws IOException {
    for (Map.Entry<String, LinkedHashSet<String>> service : services.entrySet()) {
      Path destination = staging.resolve(service.getKey()).normalize();
      if (!destination.startsWith(staging)) continue;
      Files.createDirectories(destination.getParent());
      String contents = String.join("\n", service.getValue());
      if (!contents.isEmpty()) contents += "\n";
      Files.writeString(destination, contents, StandardCharsets.UTF_8);
    }
  }

  private static String safeEntryName(String input) {
    String name = input.replace('\\', '/');
    if (name.startsWith("/") || java.util.Arrays.asList(name.split("/", -1)).contains(".."))
      return null;
    return name;
  }

  private static boolean shouldSkip(String name) {
    String upper = name.toUpperCase(java.util.Locale.ROOT);
    if (upper.equals("META-INF/MANIFEST.MF")
        || name.equals("module-info.class")
        || name.endsWith("/module-info.class")) return true;
    if (!upper.startsWith("META-INF/")) return false;
    return upper.endsWith(".SF")
        || upper.endsWith(".RSA")
        || upper.endsWith(".DSA")
        || upper.endsWith(".EC");
  }

  private static boolean isService(String name) {
    return name.startsWith("META-INF/services/");
  }

  private static void deleteTree(Path root) throws IOException {
    if (!Files.exists(root)) return;
    try (var paths = Files.walk(root)) {
      for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
    }
  }
}
