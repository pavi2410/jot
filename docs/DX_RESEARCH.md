# DX Research: Cargo and uv

How Cargo (Rust) and uv (Python) deliver great developer experience, and what jot can learn from them.

## Cargo (Rust)

**Core philosophy: One unified tool with convention over configuration.**

### Project Initialization
- `cargo new` creates a running project in <5s: `Cargo.toml`, `src/main.rs`, git initialized
- `cargo new --lib` for libraries, `cargo init` for existing directories
- Zero additional configuration needed to build and run

### Dependency Management
- **Cargo.toml** with SemVer-aware defaults: `"1.2.3"` means `>=1.2.3, <2.0.0`
- **`cargo add`** adds deps from CLI: `cargo add serde --features derive`
- **Cargo.lock** for reproducible builds (committed for binaries, omitted for libraries)
- Path deps (`{ path = "../local-crate" }`) for local development
- Combined path+version: uses local path for dev, registry version when published
- Platform-specific deps: `[target.'cfg(windows)'.dependencies]`

### Build System
- **Incremental compilation** enabled by default in dev
- **Two profiles** cover 95% of use cases:
  - `dev`: fast compile, debug symbols, incremental
  - `release`: optimized, no debug, no incremental
- Custom profiles inherit from built-ins: `inherits = "release"`
- All artifacts cached in `target/`, shared across workspace members

### Task Running
- Purpose-built commands instead of a generic task runner:
  - `cargo run`, `cargo test`, `cargo bench`, `cargo doc --open`
- `cargo check` — type-check without codegen (2-5x faster than full build)
- `cargo test` discovers tests automatically via `#[test]` attributes

### Workspace / Monorepo
- `[workspace.dependencies]` centralizes version management
- `[workspace.package]` shares metadata (version, authors, edition)
- Single `Cargo.lock` and shared `target/` across all crates
- Members reference shared deps: `serde = { workspace = true }`

### Quality Toolchain
- `cargo check` — fast feedback loop (used by rust-analyzer)
- `cargo clippy` — linter with auto-fix suggestions
- `cargo fmt` — deterministic formatting
- `cargo doc` — generates docs from comments; doc-tests are compiled and tested

### Convention over Configuration
- Filesystem IS configuration:
  - `src/main.rs` → binary, `src/lib.rs` → library
  - `tests/` → integration tests, `examples/` → examples, `benches/` → benchmarks
- Drop a file in the right directory and it works — no declaration needed

### Extensibility
- Any binary named `cargo-foo` on PATH becomes `cargo foo`
- No plugin API needed — enables a rich ecosystem (cargo-watch, cargo-audit, cargo-nextest, etc.)

### Publishing
- `cargo publish` — one-command build, verify, package, upload to crates.io
- Immutable publishing: versions can never be overwritten, only yanked

### Key Takeaways
1. **Filesystem as configuration** eliminates boilerplate
2. **`cargo check`** makes the edit-check cycle fast despite slow full compilation
3. **Doc-tests** keep documentation and code in sync
4. **SemVer-aware defaults** (`"1.2.3"` just works) reduce cognitive load
5. **Workspace-level dep deduplication** solves monorepo version management
6. **Naming convention extensibility** (`cargo-foo`) is simple and powerful

---

## uv (Python)

**Core philosophy: Single binary that replaces 7+ tools. "Cargo for Python."**

Replaces: pip, pip-tools, virtualenv, pyenv, pipx, poetry/pdm, twine.

### Speed
- Written in Rust with async Tokio runtime + Rayon thread pool
- 10-100x faster than pip (JupyterLab cold install: 2.6s vs pip's 21.4s)
- Global module cache with Copy-on-Write and hardlinks

### Project Initialization
- `uv init` → application (pyproject.toml, main.py, .gitignore, .python-version)
- `uv init --lib` → library with `src/` layout
- `uv init --package` → installable package/CLI
- `--build-backend` selects from hatchling, flit-core, setuptools, maturin, etc.

### Dependency Management
- **`uv add`** is atomic: edits `pyproject.toml` + updates `uv.lock` + syncs environment
- Supports `--dev`, `--group <name>`, `--optional <extra>`
- **`uv.lock`** is cross-platform (lock on macOS, install on Linux), human-readable TOML
- Auto-created on first `uv run` / `uv sync` / `uv lock`

### Python Version Management
- `uv python install 3.12 3.13` — installs multiple versions
- **Auto-download**: if project needs Python 3.12 and you don't have it, uv gets it on `uv run`
- `uv python pin 3.11` writes `.python-version`

### Virtual Environments
- **Invisible by default**: `uv run`, `uv sync` auto-create `.venv/`
- No manual activation needed — `uv run pytest` just works
- `uv venv --python 3.12` for explicit creation when needed

### Task / Script Running
- `uv run` handles venv creation + dep sync + execution in one step
- **Inline script dependencies (PEP 723)**: scripts declare deps in a comment header, `uv run script.py` installs them in an isolated env
- `uv add --script script.py requests` adds inline metadata

### Tool Management
- `uvx ruff check .` — runs CLI tools ephemerally (like npx)
- `uv tool install ruff` — permanently installs to PATH in isolated env

### Workspace Support (Cargo-inspired)
- Multiple packages share a single `uv.lock` and virtual environment
- `[tool.uv.workspace] members = ["packages/*"]`
- Cross-package deps: `my-lib = { workspace = true }` in `[tool.uv.sources]`

### Publishing
- `uv build` → sdist + wheel
- `uv publish` → uploads to PyPI
- Supports Trusted Publishing on CI (zero credentials)

### Compatibility
- Drop-in `uv pip install`, `uv pip compile`, `uv pip sync` for migration from pip

### Key Takeaways
1. **Speed changes behavior** — developers stop avoiding dependency updates when installs are instant
2. **Atomic operations** (`uv add`) keep manifest, lock, and environment in sync
3. **Auto-download of the runtime** (Python itself) removes a setup step
4. **Cross-platform lockfile** — resolve once, install anywhere
5. **PEP 723 inline deps** make single-file scripts self-contained
6. **Standards-based** (`pyproject.toml` / PEP 621) — no vendor lock-in

---

## Common DX Patterns

| Pattern | Cargo | uv |
|---------|-------|----|
| Single unified tool | Yes | Yes (replaces 7+ tools) |
| Written in Rust for speed | N/A (is Rust) | Yes (10-100x faster) |
| Convention over configuration | Filesystem-based targets | pyproject.toml standard |
| Zero-config start | `cargo new` + `cargo run` | `uv init` + `uv run` |
| Lockfile | `Cargo.lock` | `uv.lock` (cross-platform) |
| Workspace / monorepo | First-class | Cargo-inspired |
| Fast feedback loop | `cargo check` | Instant installs |
| Progressive disclosure | Minimal Cargo.toml → full config | 3 commands for beginners |
| Extensibility | `cargo-foo` naming convention | pip-compatible interface |
| Atomic dep management | `cargo add` edits toml + locks | `uv add` edits toml + locks + syncs |

### The Meta-Pattern

Both tools succeed by:

1. **Being a single entry point** that replaces fragmented tooling
2. **Providing sensible defaults** that require near-zero config to get started
3. **Investing in speed as a feature** — fast tools get used more often
4. **Progressive disclosure** — simple for beginners, powerful for experts
5. **Lockfiles for reproducibility** — deterministic builds across machines
6. **First-class workspace support** — monorepos are a common reality
7. **Standards-based formats** — Cargo.toml and pyproject.toml are open, not proprietary
