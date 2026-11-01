package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class GlobalCacheTest {
  @TempDir Path temp;

  @Test
  void clearsManagedCachesAndRecreatesTheirDirectories() throws Exception {
    Path jotHome = temp.resolve(".jot");
    Path jdk = Files.createDirectories(jotHome.resolve("jdks/installed-jdk"));
    Files.writeString(jdk.resolve("release"), "jdk");
    Path artifact = jotHome.resolve("java-maven-cache/com/example/library.jar");
    Files.createDirectories(artifact.getParent());
    Files.writeString(artifact, "jar");
    Files.writeString(jotHome.resolve("user-settings.toml"), "preserve");

    GlobalCache.CleanupSummary result = GlobalCache.clear(jotHome);

    assertEquals(2, result.removedEntries());
    assertTrue(result.cacheDirectories() >= 5);
    assertTrue(Files.isDirectory(jotHome.resolve("jdks")));
    assertTrue(Files.isDirectory(jotHome.resolve("java-maven-cache")));
    assertFalse(Files.exists(jdk));
    assertFalse(Files.exists(artifact));
    assertEquals("preserve", Files.readString(jotHome.resolve("user-settings.toml")));
  }
}
