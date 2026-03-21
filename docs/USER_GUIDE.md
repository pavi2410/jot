# jot User Guide

## Project Config

jot reads `jot.toml` from the current directory or nearest parent.

### Single project

```toml
[project]
name = "my-app"
version = "0.1.0"
main-class = "com.example.Main"

[toolchains]
java = "21"

[dependencies]
jackson = "com.fasterxml.jackson.core:jackson-databind:2.18.0"
```

### Workspace

```toml
[workspace]
members = ["domain", "api", "cli"]

[toolchains]
java = "21"
```

Each member has its own `jot.toml`. External dependencies are resolved into one workspace lockfile.

## Dependency Catalogs

jot supports a root `libs.versions.toml` catalog and `catalog = "name"` references in dependency entries.

## Project Templates

Use `jot init` to scaffold starter projects:

```bash
jot init --template java-minimal my-app
jot init --template java-lib my-lib
jot init --template java-cli my-cli
jot init --template java-server my-server
jot init --template java-workspace my-workspace
```

Optional flags:

- `--group <group>` sets `[workspace].group` or project group metadata defaults
- `--package <package>` sets generated Java package names

Committed runnable samples are available under `samples/java/`:

- `samples/java/minimal-app`
- `samples/java/library`
- `samples/java/cli`
- `samples/java/webserver`
- `samples/java/multi-module-workspace`

## Common Workflows

### Init

- Minimal app: `jot init --template java-minimal my-app`
- Library: `jot init --template java-lib my-lib`
- CLI: `jot init --template java-cli my-cli`
- Webserver: `jot init --template java-server my-server`
- Workspace: `jot init --template java-workspace my-workspace`

### Build

- Single project: `jot build`
- Workspace module: `jot build --module api`

### Run

- Single project: `jot run`
- Workspace module: `jot run --module api`

### Test

- All: `jot test`
- Single module: `jot test --module domain`

### Lockfile

- Generate/update lockfile: `jot lock`

### Dev tooling

- Format: `jot fmt`
- Check formatting: `jot fmt --check`
- Lint: `jot lint`
- Audit vulnerabilities: `jot audit`
- Auto-fix direct vulnerable coords (when possible): `jot audit --fix`

## Offline Mode

`jot --offline <command>` forces cache-only operation.

Behavior:

- Resolver reads cached metadata/POM/checksum entries and cached JARs
- Toolchain reads cached JDK metadata and archives
- Missing cache entries fail with actionable messages

Recommended flow before offline usage:

1. Run `jot build` online once in each relevant project/workspace.
2. Confirm lockfile and artifact caches are populated.
3. Use `jot --offline ...` in disconnected environments.

## Troubleshooting

- Unknown module errors: check `--module` value against workspace member names.
- Missing toolchain errors: ensure `[toolchains].java` exists in effective config.
- Resolver cache misses in offline mode: rerun the same command online once.
- Lockfile write contention in CI: retry after conflicting process exits; jot now serializes lockfile writes with file locks.
