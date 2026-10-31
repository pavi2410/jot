package dev.jot.core;

import java.nio.file.Path;
import java.util.List;

public record WorkspaceProject(
    Path root, ProjectConfig config, List<WorkspaceProject> dependencies) {
  public String name() {
    return config.name();
  }
}
