package dev.jot.core.resolve;

import java.util.List;

public record MavenCoordinate(String group, String artifact, String version, String classifier) {
  public static MavenCoordinate parse(String input) {
    List<String> parts = List.of(input.split(":", -1));
    if (parts.size() < 3 || parts.size() > 4 || parts.stream().anyMatch(String::isBlank)) {
      throw new IllegalArgumentException(
          "expected group:artifact:version[:classifier], got `" + input + "`");
    }
    return new MavenCoordinate(
        parts.get(0), parts.get(1), parts.get(2), parts.size() == 4 ? parts.get(3) : "");
  }

  public String moduleKey() {
    return group + ":" + artifact + ":" + classifier;
  }

  public String notation() {
    return group + ":" + artifact + ":" + version + (classifier.isEmpty() ? "" : ":" + classifier);
  }

  public String repositoryPath(String extension) {
    String groupPath = group.replace('.', '/');
    String classifierSuffix = classifier.isEmpty() ? "" : "-" + classifier;
    return groupPath
        + "/"
        + artifact
        + "/"
        + version
        + "/"
        + artifact
        + "-"
        + version
        + classifierSuffix
        + "."
        + extension;
  }

  public String pomPath() {
    return group.replace('.', '/')
        + "/"
        + artifact
        + "/"
        + version
        + "/"
        + artifact
        + "-"
        + version
        + ".pom";
  }
}
