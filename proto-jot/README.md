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
java -jar target/bin/jot-java.jar self update --check
```

The prototype supports `init`, `build`, `run`, `test`, `bench`, `fmt`, `lint`, `doc`,
`publish`, `add`, `remove`, `deps`, `outdated`, `resolve`, `tree`, `toolchain`, `lock`, `audit`, and
`clean`, and accepts `--project`, `--module`, and global `--offline`. `jot self update`
checks GitHub releases and verifies the universal Java JAR checksum before replacing itself;
`jot self uninstall` removes the current packaged JAR. `jot clean --global`
clears managed JDK downloads and Maven artifact caches while preserving other files in `~/.jot`.
`jot init [name]` creates a Jot-native Java app; `--template java-lib` creates a
library, `java-cli` a Picocli command-line app, `java-server` a JDK HTTP server,
and `java-workspace` a three-module workspace. The `app`, `library`, `minimal`,
`cli`, `server`, and `workspace` template names remain accepted as aliases. `--group`
sets the project group and default Java package; `--package` overrides the package.
Build source directories, resource
directories, and the Java release are read from `jot.toml`. A Jot manifest may
select `project.layout = "maven"` for familiar Java source, resource, and test
paths; this only selects directory defaults and does not import a `pom.xml`.
Explicit source and resource directories override those defaults. Workspaces list their members explicitly and local dependencies use
`{ path = "../module" }`. `examples/tiny-workspace` demonstrates a two-module
build and run.

Jot selects an installed JDK under `~/.jot/jdks` when its release matches
`toolchains.java`; vendor-qualified values such as `corretto-21`, `zulu-21`,
and `oracle-21` select that vendor. Plain release values accept any matching
installed JDK and can fall back to a compatible running JDK. `jot toolchain list`
shows managed JDKs. `jot toolchain pin java@21` updates the nearest project
manifest; `jot toolchain pin java@corretto-21` pins Corretto instead. Use
`--workspace` to target the workspace manifest. `jot toolchain install java@21`
installs an Adoptium JDK into the shared Jot toolchain directory. Vendor
prefixes (`adoptium`, `temurin`, `corretto`, `zulu`, or `oracle`) select a JDK
distribution through the Adoptium API; `--offline` only succeeds when that
vendor and release are already installed.

The build cache fingerprints compiler inputs separately from package inputs.
Resource-only changes update the class output and JAR without recompiling Java
sources. Java source edits are compiled on their own when their class outputs
and binary APIs remain stable; additions, deletions, binary API changes, or
compiler and classpath changes trigger a full project compile.
Runnable projects also produce `target/bin/<project>.jar`, an executable fat JAR
that includes runtime Maven and workspace dependencies. The regular project JAR
remains at `target/<project>-<version>.jar` for publishing and reuse; run the
fat JAR with `java -jar target/bin/<project>.jar`.
On a machine with GraalVM Native Image installed, `jot build --native` also
produces `target/bin/<project>` (or `<project>.exe` on Windows). Jot uses the
configured GraalVM toolchain when present; otherwise it looks for `native-image`
under `GRAALVM_HOME`, the selected Java toolchain, then `PATH`.
Native builds target the current host and require GraalVM Native Image plus its
native C build tools.
The GraalVM release must support at least the Java release selected in
`toolchains.java`; for example, GraalVM 25 can build Java 25 or older bytecode.
GraalVM can be managed separately from the Java compiler JDK: run
`jot toolchain install graalvm@25`, then pin it in `jot.toml` with
`[toolchains] graalvm = "25"` (or use `jot toolchain pin graalvm@25`). Jot installs
the latest matching GraalVM Community release and selects it for native builds.

The Java CLI resolves pinned Maven coordinates and their compile/runtime
transitive dependencies from POMs, checking repository SHA-256 sidecars when
available. `jot lock` writes the shared version 1 `jot.lock` format with
dependency roots and artifact SHA-256 hashes. Builds, tests, and benchmarks
validate their locked artifacts; projects with benchmark sources lock JMH
artifacts too. `--offline` restricts resolution to the local cache. It caches
downloads under `~/.jot/java-maven-cache`. Maven version ranges, `latest`/`release`
selectors, and timestamped snapshots resolve through Maven metadata. Kotlin
is not implemented yet. `jot lock group:artifact:version [--depth levels]
[--output path]` can also create a lockfile for ad hoc coordinates without a
project manifest.

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
`jot publish` builds a Maven publication with the project JAR, sources, Javadocs,
POM, GPG signatures, and SHA-256 sidecars. Configure `[publish]` metadata and pass
`--repository` (or set `JOT_PUBLISH_REPOSITORY`); repositories may be local paths,
`file://` URLs, or HTTP(S) endpoints. Snapshots require `--allow-snapshot`.
`jot lint [--module name]` runs `javac -Xlint:all` and reports Java compiler warnings.
`jot bench [filter]` compiles the configured benchmark source set with JMH's annotation
processor and runs it; Maven-layout projects default to `src/bench/java`, and other
projects default to `bench`. Configure `bench.jmh-version`, `bench.source-dirs`, and
`bench.deps` in `jot.toml`; `--forks`, `--warmup`, and `--iterations` tune a run.
`examples/tiny-bench` is a runnable JMH example.
Configure a pinned formatter with `[format] java = "group:artifact:version"` and
`style = "google"` or `"aosp"`; `jot fmt` formats main and test sources, while
`jot fmt --check` verifies them without writing changes. Formatter artifacts are
included in `jot.lock`.
Main-source annotation processors can be configured in `[processors]`; test-source
processors use the parallel `[test-processors]` table. Their Maven artifacts and
options participate in their respective compile fingerprints and lockfile. A source
change triggers a full compile when processors are enabled so generated sources
cannot go stale. Test compilation is incremental when source APIs remain compatible;
removed sources and changes to the compiler, classpath, processor artifacts, or
processor options trigger the needed rebuild. Generated test sources are written to
`target/generated-test-sources`.
The Java CLI supports `build --watch`, `test --watch`, and `run --watch`. It
watches configured Java source and resource directories plus manifests, lockfiles,
and version catalogs, reruns the selected action after changes, and stops with
Ctrl+C. `build --clean --watch` cleans once before the initial build.

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
