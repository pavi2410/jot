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

The prototype supports `init`, `build`, `run`, `test`, `add`, `remove`, `deps`,
`resolve`, `lock`, and `clean`, and accepts `--project`, `--module`, and global
`--offline`.
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
`jot deps [--module name]` lists each direct dependency, scope, and lock status.
`jot outdated [--module name]` compares locked versions with Maven release metadata.
`jot tree group:artifact:version [--depth levels]` displays Maven transitive
dependencies, marking omitted scopes, optional dependencies, and cycles.
`jot tree --workspace [--module name]` shows local module edges and each
selected module's external dependency tree.
`jot resolve group:artifact:version` verifies a pinned coordinate's POM, while
`--deps` lists its effective direct dependencies.
`jot doc [--module name]` generates Java API documentation under `target/docs`.
`jot lint [--module name]` runs `javac -Xlint:all` and reports Java compiler warnings.
Configure a pinned formatter with `[format] java = "group:artifact:version"` and
`style = "google"` or `"aosp"`; `jot fmt` formats main and test sources, while
`jot fmt --check` verifies them without writing changes. Formatter artifacts are
included in `jot.lock`.

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
