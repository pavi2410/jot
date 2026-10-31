package dev.jot.cli;

import java.io.IOException;
import java.nio.file.ClosedWatchServiceException;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardWatchEventKinds;
import java.nio.file.WatchEvent;
import java.nio.file.WatchKey;
import java.nio.file.WatchService;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** Watches source and resource inputs, rerunning the selected project action after changes. */
final class ProjectWatcher {
  private static final java.time.Duration DEBOUNCE = java.time.Duration.ofMillis(120);

  private ProjectWatcher() {}

  static void watch(PlanSupplier plans, CheckedAction action) throws Exception {
    try (WatchService service = FileSystems.getDefault().newWatchService()) {
      Map<WatchKey, Path> keys = new HashMap<>();
      Set<Path> registered = new HashSet<>();
      Plan plan = plans.get();
      register(plan, service, keys, registered);
      System.out.println("watch: watching project inputs (Ctrl+C to stop)");
      execute(action);
      plan = refresh(plans, plan, service, keys, registered);

      while (true) {
        WatchKey first = service.take();
        boolean changed = collect(first, keys, registered, plan);
        WatchKey next;
        long quietUntil = System.nanoTime() + DEBOUNCE.toNanos();
        while ((next =
                service.poll(
                    Math.max(1, quietUntil - System.nanoTime()),
                    java.util.concurrent.TimeUnit.NANOSECONDS))
            != null) {
          changed |= collect(next, keys, registered, plan);
          quietUntil = System.nanoTime() + DEBOUNCE.toNanos();
        }
        if (!changed) continue;
        System.out.println("watch: change detected; running again");
        execute(action);
        plan = refresh(plans, plan, service, keys, registered);
      }
    } catch (ClosedWatchServiceException ignored) {
      // The process is shutting down, usually after Ctrl+C.
    } catch (InterruptedException interrupted) {
      Thread.currentThread().interrupt();
    }
  }

  private static boolean collect(
      WatchKey key, Map<WatchKey, Path> keys, Set<Path> registered, Plan plan) {
    Path directory = keys.get(key);
    if (directory == null) return false;
    boolean changed = false;
    for (WatchEvent<?> event : key.pollEvents()) {
      if (event.kind() == StandardWatchEventKinds.OVERFLOW) {
        changed = true;
        continue;
      }
      @SuppressWarnings("unchecked")
      WatchEvent<Path> pathEvent = (WatchEvent<Path>) event;
      Path path = directory.resolve(pathEvent.context()).toAbsolutePath().normalize();
      if (plan.relevant(path, event.kind())) changed = true;
    }
    if (!key.reset()) {
      keys.remove(key);
      registered.remove(directory);
    }
    return changed;
  }

  private static void execute(CheckedAction action) {
    try {
      action.run();
    } catch (Exception error) {
      System.err.println("error: " + Main.failureMessage(error));
    }
  }

  private static Plan refresh(
      PlanSupplier plans,
      Plan current,
      WatchService service,
      Map<WatchKey, Path> keys,
      Set<Path> registered) {
    try {
      Plan updated = plans.get();
      register(updated, service, keys, registered);
      return updated;
    } catch (Exception error) {
      System.err.println("watch: " + Main.failureMessage(error));
      return current;
    }
  }

  private static void register(
      Plan plan, WatchService service, Map<WatchKey, Path> keys, Set<Path> registered) {
    try {
      for (Path root : plan.trees()) {
        if (Files.isDirectory(root)) {
          try (var paths = Files.walk(root)) {
            for (Path directory : paths.filter(Files::isDirectory).toList()) {
              registerDirectory(directory, service, keys, registered);
            }
          }
        } else {
          registerNearestExistingParent(root, service, keys, registered);
        }
      }
      for (Path file : plan.files()) registerNearestExistingParent(file, service, keys, registered);
    } catch (IOException error) {
      System.err.println("watch: could not register an input directory: " + error.getMessage());
    }
  }

  private static void registerNearestExistingParent(
      Path path, WatchService service, Map<WatchKey, Path> keys, Set<Path> registered)
      throws IOException {
    for (Path directory = path.toAbsolutePath().normalize().getParent();
        directory != null;
        directory = directory.getParent()) {
      if (Files.isDirectory(directory)) {
        registerDirectory(directory, service, keys, registered);
        return;
      }
    }
  }

  private static void registerDirectory(
      Path directory, WatchService service, Map<WatchKey, Path> keys, Set<Path> registered)
      throws IOException {
    Path normalized = directory.toAbsolutePath().normalize();
    if (!registered.add(normalized)) return;
    WatchKey key =
        normalized.register(
            service,
            StandardWatchEventKinds.ENTRY_CREATE,
            StandardWatchEventKinds.ENTRY_MODIFY,
            StandardWatchEventKinds.ENTRY_DELETE);
    keys.put(key, normalized);
  }

  record Plan(List<Path> trees, Set<Path> files) {
    Plan {
      trees = trees.stream().map(path -> path.toAbsolutePath().normalize()).distinct().toList();
      files =
          files.stream()
              .map(path -> path.toAbsolutePath().normalize())
              .collect(java.util.stream.Collectors.toUnmodifiableSet());
    }

    boolean relevant(Path path, WatchEvent.Kind<?> kind) {
      Path changed = path.toAbsolutePath().normalize();
      if (files.contains(changed) || trees.stream().anyMatch(changed::startsWith)) return true;
      if (kind == StandardWatchEventKinds.ENTRY_CREATE
          || kind == StandardWatchEventKinds.ENTRY_DELETE) {
        return trees.stream().anyMatch(tree -> tree.startsWith(changed));
      }
      return false;
    }
  }

  @FunctionalInterface
  interface PlanSupplier {
    Plan get() throws Exception;
  }

  @FunctionalInterface
  interface CheckedAction {
    void run() throws Exception;
  }
}
