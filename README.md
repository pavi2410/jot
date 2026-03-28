# jot

**A modern Java build tool — like `cargo` for Rust or `uv` for Python.**

Single binary. Zero config to get started. Fast, deterministic builds with a lockfile.

---

## Why jot?

Java tooling has historically required installing a JDK separately, writing verbose XML or Groovy DSL, and figuring out plugins just to run a formatter or check for CVEs. jot does all of that in one binary with a simple TOML config.

| Capability | jot | Maven | Gradle |
|---|---|---|---|
| Single native binary | ✓ | ✗ | ✗ |
| Built-in JDK manager | ✓ | ✗ | ✗ |
| Lockfile with artifact hashes | ✓ | Partial | Plugin |
| Workspace path dependencies | ✓ | Multi-module | Multi-project |
| `fmt` / `lint` / `audit` built in | ✓ | Plugins | Plugins |
| Dependency add/remove CLI | ✓ | ✗ | ✗ |
| `deps` / `outdated` commands | ✓ | ✗ | Plugin |

---

## Quick Start

```bash
# 1. Create a new project
jot init my-app
cd my-app

# 2. Install and pin a JDK (done once)
jot toolchain install java@21
jot toolchain pin java@21

# 3. Add a dependency
jot add com.google.guava:guava:33.4.0-jre

# 4. Build and run
jot build
jot run
```

That's it. No POM, no settings.gradle, no wrapper scripts.

---

## Install

Download the binary for your platform from the [latest GitHub Release](https://github.com/pavi2410/jot/releases), move it onto your `PATH`, then verify:

```bash
jot --version
```

Self-manage afterward:

```bash
jot self update          # upgrade to latest release
jot self update --check  # just check, don't install
jot self uninstall       # remove the binary
```

---

## Project config (`jot.toml`)

```toml
[project]
name = "my-app"
version = "1.0.0"
main-class = "com.example.App"
layout = "flat"   # "flat" (default) or "maven"

[toolchains]
java = "21"

[dependencies]
guava = "com.google.guava:guava:33.4.0-jre"

[test-dependencies]
junit = { catalog = "junit" }   # resolved from libs.versions.toml

[publish]
license = "Apache-2.0"
description = "My published library"
url = "https://github.com/example/my-app"
scm = "https://github.com/example/my-app.git"
developer = { name = "Your Name", email = "you@example.com" }
```

---

## Project Layout

jot supports two source layouts, selected via `layout` in `[project]`.

### Flat (default)

Minimal nesting — good for single-module projects.

```
my-app/
├── src/          # main sources (Java + Kotlin)
├── test/         # test sources
├── bench/        # JMH benchmark sources
├── res/          # resources (copied to JAR root)
└── jot.toml
```

### Maven

Mirrors the Maven/Gradle convention — useful when migrating an existing project or using IDEs that expect it.

```
my-app/
├── src/
│   ├── main/
│   │   ├── java/
│   │   ├── kotlin/       # mixed projects only
│   │   └── resources/
│   ├── test/
│   │   ├── java/
│   │   └── kotlin/
│   └── bench/
│       ├── java/
│       └── kotlin/
└── jot.toml
```

Set it explicitly in `jot.toml`:

```toml
[project]
layout = "maven"   # omit or set to "flat" for the default
```

---

## Command Reference

### Project lifecycle

| Command | Description |
|---|---|
| `jot init [--template <t>] [<name>]` | Scaffold a new project (`java-minimal`, `java-lib`, `java-cli`, `java-server`, `java-workspace`) |
| `jot build [--module <name>]` | Compile sources and produce a JAR (+ fat-JAR if `main-class` is set) |
| `jot run [--module <name>] [-- <args>]` | Build and run the main class |
| `jot test [--module <name>]` | Compile and run JUnit 5 tests |
| `jot bench [--module <name>]` | Compile and run JMH benchmarks from `bench/` (flat) or `src/bench/java/` (maven) |
| `jot publish [--module <name>] [--repository <target>] [--dry-run]` | Build publish artifacts, sign them with GPG, and upload them using Maven repository layout |
| `jot clean` | Delete `target/` for the current project or workspace members |
| `jot clean --global` | Wipe the global jot cache |

### Dependencies

| Command | Description |
|---|---|
| `jot add <group:artifact:version> [--test] [--name <alias>]` | Add a Maven coordinate dependency |
| `jot add --catalog <name> [--test]` | Add a version catalog reference |
| `jot remove <name> [--test]` | Remove a dependency by alias |
| `jot deps [--module <name>]` | List direct dependencies with resolved versions |
| `jot outdated [--module <name>]` | Show which dependencies have newer versions on Maven Central |
| `jot lock [<coords...>]` | Resolve and write `jot.lock` |
| `jot tree [<coord>] [--workspace] [--module <name>]` | Print the dependency tree |
| `jot resolve <coord> [--deps]` | Resolve a single coordinate |

### Code quality

| Command | Description |
|---|---|
| `jot fmt [--check] [--module <name>]` | Auto-format Java sources (Google Java Format) |
| `jot lint [--module <name>]` | Run PMD static analysis |
| `jot audit [--fix] [--ci]` | Scan locked packages for CVEs via OSV.dev; `--fix` updates declarations; `--ci` exits non-zero on findings |
| `jot doc [--open] [--module <name>]` | Generate API docs via Dokka (supports Java + Kotlin); `--open` launches `target/docs/index.html` in the browser |

### Toolchain management

| Command | Description |
|---|---|
| `jot toolchain install <tool>@<version>` | Install a toolchain (e.g. `java@21`, `java@corretto-21`, `kotlin@2.1.0`) |
| `jot toolchain list` | List installed toolchains |
| `jot toolchain pin <tool>@<version> [--workspace]` | Pin a toolchain version in `jot.toml` |

---

## Workspaces

Put a `jot.toml` at the repo root listing members, then each sub-directory has its own `jot.toml`:

```toml
# root jot.toml
[workspace]
members = ["domain", "api", "cli"]
group = "com.example"

[toolchains]
java = "21"
```

Path dependencies between members resolve automatically:

```toml
# api/jot.toml
[dependencies]
domain = { path = "../domain" }
```

---

## Version Catalog

Share versions across modules via `libs.versions.toml` at the workspace root:

```toml
[versions]
jackson = "2.18.0"
junit   = "5.11.0"

[libraries]
jackson-databind = { module = "com.fasterxml.jackson.core:jackson-databind", version.ref = "jackson" }
junit            = { module = "org.junit.jupiter:junit-jupiter", version.ref = "junit" }
```

Reference entries in `jot.toml` as `{ catalog = "jackson-databind" }`.

---

## Offline Mode

Force fully air-gapped operation (cache only):

```bash
jot --offline build
jot --offline test
```

If something's missing from cache, jot tells you exactly what to fetch first.

---

## Publishing

`jot publish` publishes standard Maven-style artifacts:

- main JAR
- sources JAR
- javadoc JAR
- generated POM
- ASCII-armored GPG signatures
- SHA-256 sidecars

Minimal flow:

```bash
jot publish --repository file:///tmp/jot-m2 --dry-run
jot publish --repository https://repo.example.com/releases
```

Notes:

- `[project].group` is required for publishing and can be inherited from `[workspace].group`
- `[publish]` metadata is required
- `gpg` must be available on `PATH`
- use `--signing-key <key-id>` or `JOT_PUBLISH_GPG_KEY` to pick a key explicitly
- use `JOT_PUBLISH_USERNAME` and `JOT_PUBLISH_PASSWORD` for authenticated HTTP repositories

---

## Sample Projects

| Sample | Layout | Description |
|---|---|---|
| `samples/java-app` | flat | CLI app using picocli; fat JAR |
| `samples/java-lib` | flat | Library with JMH benchmarks |
| `samples/java-web` | maven | HTTP server with MapStruct annotation processing |
| `samples/java-workspace` | maven | Workspace with `domain`, `api`, `cli` modules and path dependencies |
| `samples/java-kotlin` | maven | Mixed Java + Kotlin project |
| `samples/kotlin-app` | flat | Kotlin-only app |

---

## Documentation

- [User Guide](docs/USER_GUIDE.md)
- [Design Proposal](docs/DESIGN_PROPOSAL.md)
- [Implementation Plan](docs/IMPL_PLAN.md)

---

## License

Licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

