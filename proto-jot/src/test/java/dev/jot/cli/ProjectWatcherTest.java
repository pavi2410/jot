package dev.jot.cli;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ProjectWatcherTest {
  @TempDir Path temp;

  @Test
  void rerunsActionAfterAnInputChangesAndStopsWhenInterrupted() throws Exception {
    Path sources = Files.createDirectories(temp.resolve("src/main/java"));
    Path source = sources.resolve("App.java");
    Files.writeString(source, "class App {}\n");
    CountDownLatch initialRun = new CountDownLatch(1);
    CountDownLatch secondRun = new CountDownLatch(1);
    AtomicInteger runs = new AtomicInteger();
    Thread watcher =
        new Thread(
            () -> {
              try {
                ProjectWatcher.watch(
                    () -> new ProjectWatcher.Plan(List.of(sources), Set.of()),
                    () -> {
                      if (runs.incrementAndGet() == 1) initialRun.countDown();
                      else secondRun.countDown();
                    });
              } catch (Exception error) {
                throw new AssertionError(error);
              }
            });
    watcher.setDaemon(true);
    watcher.start();

    assertTrue(initialRun.await(5, TimeUnit.SECONDS));
    Files.writeString(source, "class App { int value() { return 1; } }\n");
    try {
      assertTrue(secondRun.await(5, TimeUnit.SECONDS));
    } finally {
      watcher.interrupt();
      watcher.join(5000);
    }
    assertFalse(watcher.isAlive(), "interruption should stop the watch loop");
  }
}
