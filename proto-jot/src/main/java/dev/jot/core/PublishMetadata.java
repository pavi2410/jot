package dev.jot.core;

/** Required Maven repository metadata supplied by a project's {@code [publish]} table. */
public record PublishMetadata(
    String license, String description, String url, String scm, Developer developer) {
  public record Developer(String name, String email) {}
}
