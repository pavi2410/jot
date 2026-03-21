# jot

A Rust-native JVM toolchain manager focused on fast, deterministic Java builds.

## What jot does

- Manages JDK installs and project pinning
- Resolves Maven dependencies with lockfile output
- Builds and runs single-project and workspace modules
- Supports workspace-aware formatting, linting, and dependency auditing

## Quick Start

1. Build the CLI:

```bash
cargo build -p jot
```

2. Initialize or enter a project with a `jot.toml` file.

3. Install a JDK and pin it:

```bash
jot java install 21
jot java pin 21
```

4. Build and run:

```bash
jot build
jot run
```

## Java Samples

Runnable sample projects live in `samples/java`:

- `samples/java/minimal-app` - simple single-project app
- `samples/java/library` - library-only project
- `samples/java/cli` - command-line app
- `samples/java/webserver` - lightweight HTTP server
- `samples/java/multi-module-workspace` - workspace with `domain`, `api`, and `cli`

You can scaffold the same shapes with `jot init`:

```bash
jot init --template java-minimal my-app
jot init --template java-lib my-lib
jot init --template java-cli my-cli
jot init --template java-server my-server
jot init --template java-workspace my-workspace
```

## Why jot vs Maven/Gradle

| Capability | jot | Maven | Gradle |
|---|---|---|---|
| Native single binary | Yes | No | No |
| Built-in JDK manager | Yes | No | No |
| Lockfile with artifact hashes | Yes | Partial | Plugin-dependent |
| Workspace path deps | Yes | Multi-module model | Multi-project model |
| Built-in fmt/lint/audit commands | Yes | Plugins | Plugins |

## Command Reference

- `jot init [--template <java-minimal|java-lib|java-cli|java-server|java-workspace>] [--group <group>] [--package <package>] [<name>]`
- `jot build [--module <name>]`
- `jot run [--module <name>] [-- <args...>]`
- `jot test [--module <name>]`
- `jot lock [<group:artifact[:version]>...]`
- `jot tree [<group:artifact[:version]>] [--workspace] [--module <name>]`
- `jot fmt [--check] [--module <name>]`
- `jot lint [--module <name>]`
- `jot audit [--fix] [--ci]`
- `jot java install <version> [--vendor <adoptium|corretto|zulu|oracle>]`
- `jot java list`
- `jot java pin <version> [--vendor <vendor>] [--workspace]`
- `jot clean --global`

## Offline Mode

Use offline mode to force cache-only behavior:

```bash
jot --offline build
jot --offline test
jot --offline audit
```

When required metadata, JARs, or JDK archives are missing from cache, jot now returns explicit guidance to run the command online once.

## Documentation

- User guide: `docs/USER_GUIDE.md`
- Implementation roadmap: `IMPL_PLAN.md`
- Design proposal: `DESIGN_PROPOSAL.md`
