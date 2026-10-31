package dev.jot.core;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.util.List;

/** Clears Jot-managed toolchain and dependency caches without removing the Jot home directory. */
public final class GlobalCache {
  private static final List<String> CACHE_DIRECTORIES =
      List.of("jdks", "kotlins", "downloads", "resolve-cache", "locks", "java-maven-cache");

  private GlobalCache() {}

  public static CleanupSummary clear(Path jotHome) throws IOException {
    Path root = jotHome.toAbsolutePath().normalize();
    int removedEntries = 0;
    for (String name : CACHE_DIRECTORIES) {
      Path directory = root.resolve(name);
      if (Files.isDirectory(directory, LinkOption.NOFOLLOW_LINKS)) {
        try (var children = Files.list(directory)) {
          for (Path child : children.toList()) {
            deleteTree(child);
            removedEntries++;
          }
        }
      }
      Files.createDirectories(directory);
    }
    return new CleanupSummary(removedEntries, CACHE_DIRECTORIES.size());
  }

  private static void deleteTree(Path path) throws IOException {
    if (Files.isDirectory(path, LinkOption.NOFOLLOW_LINKS)) {
      try (var children = Files.list(path)) {
        for (Path child : children.toList()) deleteTree(child);
      }
    }
    Files.deleteIfExists(path);
  }

  public record CleanupSummary(int removedEntries, int cacheDirectories) {}
}
