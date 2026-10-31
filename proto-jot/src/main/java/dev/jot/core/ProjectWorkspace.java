package dev.jot.core;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** Loads workspace members and orders them by their declared local dependencies. */
public record ProjectWorkspace(List<WorkspaceProject> ordered) {
  public static ProjectWorkspace load(Path root, ProjectConfig rootConfig) throws IOException {
    List<WorkspaceProject> ordered = new ArrayList<>();
    Map<Path, WorkspaceProject> byPath = new TreeMap<>();
    List<Path> visiting = new ArrayList<>();
    for (String member : rootConfig.workspaceMembers()) {
      visit(
          root.resolve(member),
          rootConfig.javaSpecification(),
          rootConfig.group(),
          visiting,
          byPath,
          ordered);
    }
    return new ProjectWorkspace(List.copyOf(ordered));
  }

  public static ProjectWorkspace loadProject(Path root, ProjectConfig config) throws IOException {
    List<WorkspaceProject> ordered = new ArrayList<>();
    Map<Path, WorkspaceProject> byPath = new TreeMap<>();
    List<Path> visiting = new ArrayList<>();
    visitProject(root, config, visiting, byPath, ordered);
    return new ProjectWorkspace(List.copyOf(ordered));
  }

  private static WorkspaceProject visitProject(
      Path projectRoot,
      ProjectConfig config,
      List<Path> visiting,
      Map<Path, WorkspaceProject> byPath,
      List<WorkspaceProject> ordered)
      throws IOException {
    Path normalizedRoot = projectRoot.toAbsolutePath().normalize();
    WorkspaceProject known = byPath.get(normalizedRoot);
    if (known != null) return known;
    if (visiting.contains(normalizedRoot)) {
      throw new IllegalArgumentException("workspace path dependency cycle at " + normalizedRoot);
    }
    visiting.add(normalizedRoot);
    List<WorkspaceProject> dependencies = new ArrayList<>();
    for (String dependencyPath : config.pathDependencies().values()) {
      Path dependencyRoot = normalizedRoot.resolve(dependencyPath);
      Path manifest = dependencyRoot.resolve("jot.toml");
      if (!Files.isRegularFile(manifest)) {
        throw new IllegalArgumentException("missing project manifest: " + manifest);
      }
      ProjectConfig dependencyConfig =
          ProjectConfig.read(manifest, config.javaSpecification(), config.group());
      dependencies.add(visitProject(dependencyRoot, dependencyConfig, visiting, byPath, ordered));
    }
    visiting.removeLast();
    WorkspaceProject project =
        new WorkspaceProject(normalizedRoot, config, List.copyOf(dependencies));
    byPath.put(normalizedRoot, project);
    ordered.add(project);
    return project;
  }

  private static WorkspaceProject visit(
      Path projectRoot,
      String inheritedJava,
      String inheritedGroup,
      List<Path> visiting,
      Map<Path, WorkspaceProject> byPath,
      List<WorkspaceProject> ordered)
      throws IOException {
    Path normalizedRoot = projectRoot.toAbsolutePath().normalize();
    WorkspaceProject known = byPath.get(normalizedRoot);
    if (known != null) return known;
    if (visiting.contains(normalizedRoot)) {
      List<String> cycle = new ArrayList<>();
      for (Path path : visiting) cycle.add(path.getFileName().toString());
      cycle.add(normalizedRoot.getFileName().toString());
      throw new IllegalArgumentException(
          "workspace path dependency cycle: " + String.join(" -> ", cycle));
    }
    Path manifest = normalizedRoot.resolve("jot.toml");
    if (!Files.isRegularFile(manifest)) {
      throw new IllegalArgumentException("missing project manifest: " + manifest);
    }
    ProjectConfig config = ProjectConfig.read(manifest, inheritedJava, inheritedGroup);
    visiting.add(normalizedRoot);
    List<WorkspaceProject> dependencies = new ArrayList<>();
    for (String dependencyPath : config.pathDependencies().values()) {
      dependencies.add(
          visit(
              normalizedRoot.resolve(dependencyPath),
              config.javaSpecification(),
              config.group(),
              visiting,
              byPath,
              ordered));
    }
    visiting.removeLast();
    WorkspaceProject project =
        new WorkspaceProject(normalizedRoot, config, List.copyOf(dependencies));
    byPath.put(normalizedRoot, project);
    ordered.add(project);
    return project;
  }

  public WorkspaceProject find(String name) {
    return ordered.stream()
        .filter(
            project ->
                project.name().equals(name) || project.root().getFileName().toString().equals(name))
        .findFirst()
        .orElseThrow(() -> new IllegalArgumentException("unknown workspace module `" + name + "`"));
  }

  public WorkspaceProject selectRunnable(String selectedModule) {
    if (selectedModule != null) {
      WorkspaceProject selected = find(selectedModule);
      if (selected.config().mainClass().isBlank()) {
        throw new IllegalArgumentException(
            "workspace module `" + selectedModule + "` has no project.main-class");
      }
      return selected;
    }
    List<WorkspaceProject> runnable =
        ordered.stream().filter(project -> !project.config().mainClass().isBlank()).toList();
    if (runnable.size() != 1) {
      throw new IllegalArgumentException(
          "workspace `jot run` needs --module when it has "
              + runnable.size()
              + " runnable modules");
    }
    return runnable.getFirst();
  }

  public List<WorkspaceProject> dependenciesFirst(WorkspaceProject project) {
    List<WorkspaceProject> result = new ArrayList<>();
    appendDependenciesFirst(project, new HashSet<>(), result);
    return result;
  }

  private static void appendDependenciesFirst(
      WorkspaceProject project, HashSet<Path> seen, List<WorkspaceProject> result) {
    if (!seen.add(project.root())) return;
    for (WorkspaceProject dependency : project.dependencies()) {
      appendDependenciesFirst(dependency, seen, result);
    }
    result.add(project);
  }
}
