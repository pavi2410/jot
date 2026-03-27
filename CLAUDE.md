# CLAUDE.md

This file provides guidance for AI assistants working on the `jot` codebase.

## Project Overview

**jot** is a modern Java/Kotlin build tool written in Rust — think Cargo for the JVM ecosystem. It manages dependencies from Maven Central, compiles Java and Kotlin code, packages JARs, manages JDK toolchains, and provides code quality tools (format, lint, audit), all as a single static binary.

## Repository Structure

```
jot/
├── crates/                    # Rust workspace (9 crates)
│   ├── jot/                   # Main CLI binary
│   ├── jot-builder/           # Compilation & JAR packaging
│   ├── jot-resolver/          # Maven dependency resolution
│   ├── jot-config/            # jot.toml parsing
│   ├── jot-devtools/          # Format, lint, audit tooling
│   ├── jot-toolchain/         # JDK/Kotlin version management
│   ├── jot-cache/             # Cache directory (~/.jot) management
│   ├── jot-common/            # Shared utilities (archives, locks, progress)
│   └── jot-platform/          # OS/platform detection
├── samples/                   # Integration test projects (Java + Kotlin)
├── docs/                      # Design docs, ADRs, user guide
├── .github/workflows/         # CI (ci.yml) and release (release.yml)
├── Cargo.toml                 # Workspace manifest
└── README.md
```

## Crate Responsibilities

| Crate | Key Files | Responsibility |
|-------|-----------|----------------|
| `jot` | `src/cli.rs`, `src/commands/` | CLI parsing (clap), command dispatch |
| `jot-builder` | `src/lib.rs`, `src/compile/`, `src/package.rs` | Invoke javac/kotlinc, assemble JARs |
| `jot-resolver` | `src/resolver.rs`, `src/versions.rs` | Maven Central resolution, PubGrub algorithm, lockfile |
| `jot-config` | `src/lib.rs`, `src/dependencies.rs`, `src/editing.rs` | Parse/edit `jot.toml`, workspace discovery |
| `jot-devtools` | `src/lib.rs`, `src/audit.rs`, `src/format/`, `src/lint/` | Wrap Google Java Format, PMD, KtLint, Detekt, OSV.dev CVE scan |
| `jot-toolchain` | `src/lib.rs` | Download/manage JDK vendors (Adoptium, Corretto, Zulu, Oracle) and Kotlin |
| `jot-cache` | `src/lib.rs` | Canonical paths under `~/.jot/` |
| `jot-common` | `src/archive.rs`, `src/lock.rs` | TAR/ZIP extraction, file locking, progress bars |
| `jot-platform` | `src/lib.rs` | OS/arch detection for download URL construction |

## Build & Development Commands

```bash
# Build all crates
cargo build --workspace

# Build release binary
cargo build --release

# Run all tests
cargo test --locked --workspace --verbose

# Test a specific crate
cargo test -p jot-resolver

# Run the CLI directly
cargo run -- <subcommand>
```

## Key Conventions

### Error Handling
- Use `thiserror` to define typed errors per crate (e.g., `BuildError`, `ResolveError`).
- At call sites, wrap with `anyhow` (`?` propagation) for ergonomic error messages.
- Do not use `.unwrap()` in library code; use `.expect("reason")` only in test code.

### Progress & Output
- All long-running operations must use `indicatif` for progress indicators.
- Use `yansi` for ANSI color output. Check terminal capability before coloring.
- Helpers for spinners and download bars live in `jot-common`.

### Path Handling
- Always use `PathBuf` / `std::path::Path`; never concatenate paths as strings.
- Cache paths (e.g., downloaded JDKs, tool JARs) must go through `jot-cache`.

### Module Structure
- Public API is exported from `lib.rs`; internal logic lives in submodules.
- Keep cross-crate dependencies minimal. Prefer `jot-common` for utilities used by multiple crates.

### Configuration (`jot.toml`)
- Parsing and data structures live in `jot-config`.
- Editing the TOML in-place (e.g., `jot add`, `jot pin`) uses `toml_edit` via `crates/jot-config/src/editing.rs`.
- Follow Maven directory conventions for source roots: `src/main/java`, `src/test/java`, `src/main/kotlin`, `src/test/kotlin`, `src/main/resources`.

### Dependency Resolution
- Uses the PubGrub algorithm (via the `pubgrub` crate) for version conflict resolution.
- Lockfile is `jot.lock` at project root; always respect it when present.
- Maven coordinates are `group:artifact:version[:classifier]` — see `jot-resolver/src/coordinate.rs`.

### Compilation Pipeline Order
1. Resolve dependencies (check lockfile or resolve fresh).
2. Collect Java and Kotlin sources.
3. If mixed language, compile Kotlin first, then Java.
4. Copy resources.
5. Package JAR; if `main-class` is set, also build a fat JAR.

### Tool JARs (Devtools)
Tool JARs (Google Java Format, PMD, KtLint, Detekt, etc.) are downloaded on first use and cached under `~/.jot/tools/`. Version constants are in `jot-devtools/src/lib.rs`.

## CI/CD

- **`ci.yml`**: Runs on every push/PR to `main`. Steps: build → test → build all samples (matrix: Linux + macOS).
- **`release.yml`**: Triggered on version tags. Builds for 4 targets (Linux x86\_64 musl, macOS x86\_64, macOS ARM64, Windows x86\_64), signs macOS binaries, produces SHA256SUMS, and publishes to GitHub Releases.

Release profile enables `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"` for minimal binary size.

## Sample Projects

`samples/` contains working projects used as integration tests in CI. Each has a `jot.toml` and is independently buildable:

- `java/minimal-app` — Hello World
- `java/library` — Library (no main class)
- `java/cli` — CLI app with dependencies
- `java/webserver` — HTTP server
- `java/multi-module-workspace` — Workspace with multiple modules
- `java/annotation-processing` — APT example
- `kotlin/minimal-app` — Kotlin Hello World
- `kotlin/mixed-java-kotlin` — Mixed Java + Kotlin

When adding new features, update the relevant sample or add a new one to verify end-to-end behavior.

## Documentation

| File | Purpose |
|------|---------|
| `README.md` | User-facing overview and quick-start |
| `docs/USER_GUIDE.md` | Detailed usage |
| `docs/DESIGN_PROPOSAL.md` | Vision, design principles, architecture rationale |
| `docs/IMPL_PLAN.md` | Implementation roadmap and status |
| `docs/DX_RESEARCH.md` | Developer experience comparisons (Cargo, uv, Maven, Gradle) |
| `docs/adr/` | Architecture Decision Records |

When making significant architectural decisions, add an ADR under `docs/adr/`.

## Adding a New CLI Command

1. Create `crates/jot/src/commands/<name>.rs`.
2. Add the subcommand variant to the `Cli` enum in `crates/jot/src/cli.rs`.
3. Dispatch it in `crates/jot/src/commands/mod.rs`.
4. If the command needs new logic, add it to the appropriate domain crate (e.g., `jot-builder`), not inline in the CLI command.

## Dependency Management (Rust)

Shared dependencies are declared under `[workspace.dependencies]` in the root `Cargo.toml` and referenced in crate manifests with `{ workspace = true }`. When adding a new dependency:
- If used by more than one crate, add it to the workspace manifest.
- Prefer existing workspace dependencies over adding new ones.

## Rust Edition & MSRV

- Edition: **2024**
- License: MIT OR Apache-2.0
- Version: 0.1.0 (early development)
