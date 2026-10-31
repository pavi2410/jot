# proto-Jot

This Java prototype checks whether Jot can build a Java implementation of
itself. It reads `jot.toml` with Tomlj, compiles Java sources with `javac`, and
packages JARs with `jar`.

## Bootstrap and self-build

From the repository root, install the recent Java toolchain with the current
Rust Jot implementation, then use it to build the prototype:

```sh
cargo run -p jot --manifest-path Cargo.toml -- toolchain install java@27
cd proto-jot
cargo run --manifest-path ../Cargo.toml -p jot -- build
```

For the Java Jot commands below, use the managed Java 27 installation as
`JAVA_HOME` and put `$JAVA_HOME/bin` first on `PATH`.

The Java implementation is one project, organized into packages for the CLI,
configuration, workspace loading, and build execution. TOML parsing and writing
and CLI parsing stay isolated from build logic. Use the resulting Java Jot to build
itself:

```sh
java -jar target/bin/jot-java.jar build
java -jar target/bin/jot-java.jar run -- --version
```

The prototype supports `init`, `build`, `run`, `test`, `add`, `remove`, `lock`,
and `clean`, and accepts `--project`, `--module`, and global `--offline`.
`jot init [name]` creates a Jot-native
Java app; `--template library` creates a library, and `--package` sets its Java
package. Build source directories, resource
directories, and the Java release are read from `jot.toml`. A Jot manifest may
select `project.layout = "maven"` for familiar Java source, resource, and test
paths; this only selects directory defaults and does not import a `pom.xml`.
Explicit source and resource directories override those defaults. Workspaces list their members explicitly and local dependencies use
`{ path = "../module" }`. `examples/tiny-workspace` demonstrates a two-module
build and run.

Jot currently uses the JDK it is running on. That JDK must be at least as new
as the Java release requested by the project; Jot reports a direct error when
it is too old.

The build cache fingerprints compiler inputs separately from package inputs.
Resource-only changes update the class output and JAR without recompiling Java
sources. Java source edits are compiled on their own when their class outputs
and binary APIs remain stable; additions, deletions, binary API changes, or
compiler and classpath changes trigger a full project compile.

The Java CLI resolves pinned Maven coordinates and their compile/runtime
transitive dependencies from POMs, checking repository SHA-256 sidecars when
available. `jot lock` writes the shared version 1 `jot.lock` format with
dependency roots and artifact SHA-256 hashes. Builds and tests validate locked
artifacts, while `--offline` restricts resolution to the local cache. It caches
downloads under `~/.jot/java-maven-cache`. Version ranges, snapshots, and Kotlin
are not implemented yet.

`jot add group:artifact:version [--name alias] [--test]` or
`jot add --catalog alias [--name dependency] [--test]`, and
`jot remove alias [--test]` edit dependency tables while retaining other
manifest content, then regenerate the lockfile. Catalog aliases resolve from
the nearest `libs.versions.toml` up the project path.

The Java CLI includes a `test` command that compiles configured test sources and
runs JUnit Platform Console. The prototype's own tests run through that command:

```sh
java -jar target/bin/jot-java.jar test
java -jar target/bin/jot-java.jar lock
java -jar target/bin/jot-java.jar test --offline
```

The TOML reader supports TOML 1.0 through Tomlj. The document writer remains
isolated for generated manifests and lockfiles.

```sh
java -jar target/bin/jot-java.jar build --project examples/tiny-workspace
java -jar target/bin/jot-java.jar run --project examples/tiny-workspace --module app
```
