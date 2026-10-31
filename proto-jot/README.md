# proto-Jot

This small Java prototype checks whether Jot can build a Java implementation
of itself. It reads a narrow subset of `jot.toml`, compiles Java sources with
`javac`, and packages an executable JAR with `jar`. It has no external runtime
dependencies.

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
configuration, workspace loading, and build execution. The TOML reader/writer
and CLI parser are isolated classes, so they can be replaced by dedicated Java
libraries without moving build logic. Use the resulting Java Jot to build
itself:

```sh
java -jar target/bin/jot-java.jar build
java -jar target/bin/jot-java.jar run -- --version
```

The prototype supports `build`, `run`, `test`, `lock`, and `clean`, and accepts
`--project`, `--module`, and `--offline`. Build source directories, resource
directories, and the Java release are read from `jot.toml`. A Jot manifest may
select `project.layout = "maven"` for familiar Java source, resource, and test
paths; this only selects directory defaults and does not import a `pom.xml`.
Explicit source and resource directories override those defaults. Workspaces list their members explicitly and local dependencies use
`{ path = "../module" }`. `examples/tiny-workspace` demonstrates a two-module
build and run.

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
downloads under `~/.jot/java-maven-cache`. Version ranges,
snapshots, Kotlin, and the full TOML specification are not implemented yet.

The Java CLI includes a `test` command that compiles configured test sources and
runs JUnit Platform Console. The prototype's own tests run through that command:

```sh
java -jar target/bin/jot-java.jar test
java -jar target/bin/jot-java.jar lock
java -jar target/bin/jot-java.jar test --offline
```

The bootstrap TOML reader currently handles strings, integers, string arrays,
inline tables, and array tables. It remains a deliberately small subset of TOML.

```sh
java -jar target/bin/jot-java.jar build --project examples/tiny-workspace
java -jar target/bin/jot-java.jar run --project examples/tiny-workspace --module app
```
