package dev.jot.core;

import java.util.List;

/** JMH inputs for a project's explicitly configured benchmark source set. */
public record BenchmarkConfig(
    String jmhVersion, List<String> sourceDirectories, List<String> dependencies) {
  public static final String DEFAULT_JMH_VERSION = "1.37";

  public BenchmarkConfig {
    if (jmhVersion == null || jmhVersion.isBlank()) {
      throw new IllegalArgumentException("bench.jmh-version cannot be blank");
    }
    sourceDirectories = List.copyOf(sourceDirectories);
    dependencies = List.copyOf(dependencies);
  }
}
