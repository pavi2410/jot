# `jot` Implementation Plan — Solo Full-Time (v3)

**Context:** Solo developer, intermediate Rust, targeting production-grade quality, Windows/macOS/Linux support intended. CI runs on Linux only to conserve GitHub Actions minutes — manual platform testing at phase boundaries.

**Core strategic decision:** Ship a Java-only build tool with full workspace support. Defer Kotlin, plugins, and observability. The demo is a multi-module workspace that builds a library, a web server, and a CLI tool in one command.

---

## The MVP Scope

What ships in v0.1:

- Single-project and workspace Java compilation and running
- JDK management (download, pin, switch)
- PubGrub dependency resolution against Maven Central
- `jot.lock` with SHA-256 verification (single lockfile per workspace)
- `jot.toml` config with full workspace support
- `libs.versions.toml` catalog support (including BOMs), shared across workspace
- Toolchain inheritance with per-module override
- Path dependencies between workspace members
- `[[bin]]` with `fat-jar` distribution
- `jot fmt` / `jot lint` (hardcoded, no plugin system) — workspace-aware
- `jot init` with templates: `java-lib`, `java-cli`, `java-server`, `java-workspace`
- Cross-platform binaries: Linux, macOS (Intel + ARM), Windows. CI tested on Linux only
- `jot add` / `jot remove` / `jot tree` / `jot audit`
- `jot test` via JUnit 5

What's explicitly deferred to post-v0.1:

- Kotlin support
- Plugin system (all features are hardcoded internal)
- Observability (inspect, profile, doctor --runtime)
- `native-image` and `jlink` dist formats
- `jot bench`
- `jot tool run` / `jot tool install`
- `jot publish`
- `jot import` (no migration — greenfield only)
- Incremental compilation
- Spring Boot support (requires custom repackaging)

---

## Demo Target: The `shopflow` Workspace

The v0.1 launch demonstrates jot with a single workspace containing three modules. This is the demo that sells jot to real teams.

```
shopflow/
├── jot.toml                    # workspace root
├── jot.lock                    # single lockfile
├── libs.versions.toml          # shared catalog
├── domain/
│   ├── jot.toml
│   └── src/main/java/
├── api/
│   ├── jot.toml
│   └── src/main/java/
└── csvq/
    ├── jot.toml
    └── src/main/java/
```

### Workspace Root

```toml
[workspace]
members = ["domain", "api", "csvq"]
group = "com.shopflow"

[toolchains]
java = "21"
```

### 1. `domain` — Pure Java Library

A domain model JAR with no main class. Depends on Jackson for serialization.

```toml
[project]
name = "shopflow-domain"
version = "1.0.0"

[dependencies]
jackson-databind = { catalog = "jackson-databind" }

[test-dependencies]
junit = { catalog = "junit" }
```

**What it proves:** Library compilation, JAR packaging (no fat-jar), test execution, transitive dependency resolution (Jackson pulls in jackson-core, jackson-annotations), catalog references.

### 2. `api` — Helidon SE Web Server

A JSON REST API using Helidon SE. Depends on `domain` via path dep.

```toml
[project]
name = "shopflow-api"
version = "1.0.0"
main-class = "com.shopflow.api.Main"

[dependencies]
domain = { path = "../domain" }
helidon-webserver = { catalog = "helidon-webserver" }
helidon-media-jsonp = { catalog = "helidon-media-jsonp" }

[test-dependencies]
junit = { catalog = "junit" }
helidon-testing = { catalog = "helidon-testing" }
```

**What it proves:** Path dependencies between workspace members, fat-jar packaging, real dependency tree (~30 transitive deps), `jot run` starts a working HTTP server, build ordering (domain compiles before api).

**Why Helidon SE over Spring Boot:** Helidon SE is a plain Java library — no annotation scanning, no custom classloader, no repackaging magic. A standard shade-style fat-jar works out of the box. Spring Boot requires a custom nested-JAR layout which doesn't belong in MVP.

### 3. `csvq` — CLI Tool with Picocli

A command-line tool. Independent module in the workspace (no path deps on other members).

```toml
[project]
name = "csvq"
version = "0.5.0"
main-class = "dev.csvq.Main"

[dependencies]
picocli = { catalog = "picocli" }

[processors]
picocli-codegen = "info.picocli:picocli-codegen:4.7.6"

[test-dependencies]
junit = { catalog = "junit" }
```

**What it proves:** Annotation processor support, workspace member with no internal deps (builds in parallel with domain), fat-jar as CLI executable, `jot run -- --help` passes args correctly.

### Shared Catalog: `libs.versions.toml`

```toml
[versions]
jackson = "2.18.0"
helidon = "4.1.6"
junit = "5.11.0"
picocli = "4.7.6"

[libraries]
jackson-databind = { module = "com.fasterxml.jackson.core:jackson-databind", version.ref = "jackson" }
helidon-webserver = { module = "io.helidon.webserver:helidon-webserver", version.ref = "helidon" }
helidon-media-jsonp = { module = "io.helidon.http.media:helidon-http-media-jsonp", version.ref = "helidon" }
helidon-testing = { module = "io.helidon.webserver.testing:helidon-webserver-testing-junit5", version.ref = "helidon" }
junit = { module = "org.junit.jupiter:junit-jupiter", version.ref = "junit" }
picocli = { module = "info.picocli:picocli", version.ref = "picocli" }
```

### The Demo Moment

```bash
$ curl -fsSL https://jot.dev/install.sh | sh
$ jot init --template java-workspace shopflow && cd shopflow
$ jot build
  → domain (java 21, javac)... target/shopflow-domain-1.0.0.jar
  → csvq (java 21, javac)... target/bin/csvq (fat-jar)
  → api (java 21, javac)... target/bin/shopflow-api (fat-jar)
  Built 3 modules in 4.2s

$ jot test
  → domain: 5 tests passed (0.8s)
  → csvq: 3 tests passed (0.6s)
  → api: 4 tests passed (1.2s)

$ jot run --module api
  → Started shopflow-api on port 8080

$ jot fmt && jot lint && jot audit
  → 3 modules formatted. 0 lint issues. 0 vulnerabilities.
```

---

## Architecture Overview

```
┌──────────────────────────────────────────────────┐
│                 jot CLI (Rust)                     │
│              Single static binary                  │
├──────────┬───────────┬───────────┬────────────────┤
│ Config   │ Toolchain │ Resolver  │ Builder        │
│ Parser   │ Manager   │           │                │
├──────────┼───────────┼───────────┼────────────────┤
│ jot.toml │ JDK dl/   │ pubgrub-  │ javac          │
│ TOML     │ pin/      │ rs +      │ orchestration  │
│ catalog  │ switch    │ Maven     │ fat-jar        │
│ parsing  │           │ Central   │ packaging      │
│ workspace│           │ client    │ workspace      │
│ graph    │           │           │ build DAG      │
├──────────┴───────────┴───────────┴────────────────┤
│            Global Cache (~/.jot/)                   │
│   Content-addressable: JDKs | JARs | resolve cache │
└──────────────────────────────────────────────────┘
```

No JVM plugin host in MVP. Formatting and linting invoke external tool JARs directly via `java -jar`. This eliminates the entire IPC complexity.

---

## Phase 0: Skeleton & CI (Weeks 1–2)

**Goal:** Repo structure, Linux CI, cross-compiled binaries for all platforms, `jot --version` ships.

### Week 1: Project Bootstrap

| Task | Details | Est. |
|------|---------|------|
| Cargo workspace setup | `jot-cli`, `jot-config`, `jot-resolver`, `jot-toolchain`, `jot-builder`, `jot-cache` crates | 1 day |
| CLI framework | `clap` v4 with derive macros. Stub all subcommands (init, run, build, add, remove, etc.) | 1 day |
| CI pipeline | GitHub Actions on Linux only: build + test `x86_64-unknown-linux-musl`. All CI minutes spent on one runner | 1 day |
| Release pipeline | `cargo-dist` or manual GH Actions for tagged releases. Cross-compiles on Linux to produce binaries for: `x86_64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` | 1 day |
| Install scripts | `install.sh` (curl-pipe-sh) + `install.ps1` (PowerShell for Windows) | 0.5 day |

### Week 2: Config Parser Foundation

| Task | Details | Est. |
|------|---------|------|
| `jot.toml` parser | `toml` crate + `serde` deserialization into typed config structs. Cover: `[project]`, `[workspace]`, `[toolchains]`, `[dependencies]`, `[test-dependencies]`, `[processors]`, `[[bin]]`, `[format]`, `[lint]` | 2 days |
| `libs.versions.toml` parser | Parse Gradle catalog format: `[versions]`, `[libraries]`, `[bundles]`. Map `catalog = "..."` refs to resolved coordinates | 2 days |
| Config validation | Error on missing `java` in toolchains, unknown keys, invalid version formats. Human-readable error messages with file:line:col | 1 day |

**Deliverable:** `jot --version` works on Linux. Cross-compiled binaries produced for macOS and Windows (untested in CI — manual verification at phase boundaries). Config parser fully tested.

---

## Phase 1: JDK Management (Weeks 3–5)

**Goal:** `jot java install`, `jot java list`, `jot java pin`. jot manages JDKs without requiring any pre-installed Java.

### Week 3: JDK Discovery & Download

| Task | Details | Est. |
|------|---------|------|
| Adoptium API client | Query `api.adoptium.net` for available JDK versions. Parse response, filter by OS/arch. Support `adoptium` (default), `corretto`, `zulu`, `oracle` via their respective APIs | 3 days |
| Download & extract | HTTP/2 via `reqwest`, progress bar via `indicatif`. Extract `.tar.gz`. Store in `~/.jot/jdks/<vendor>-<version>/` | 2 days |

### Week 4: JDK Pinning & Resolution

| Task | Details | Est. |
|------|---------|------|
| Version resolution | `java = "21"` resolves to latest `21.x.y` patch from Adoptium. Cache version manifest locally with TTL | 1 day |
| Per-project pinning | `jot java pin 21` writes to nearest `jot.toml`. `jot run` reads `[toolchains].java`, auto-downloads if missing | 1 day |
| PATH shimming | `jot` sets `JAVA_HOME` and prepends JDK `bin/` to PATH for child processes. No global PATH modification | 1 day |
| `jot java list` | Show installed JDKs, mark active one for current project | 0.5 day |
| Platform detection | Detect arch at runtime (x86_64 vs aarch64). Map to Adoptium/Corretto/Zulu download URLs | 0.5 day |

### Week 5: Cache & Cleanup

| Task | Details | Est. |
|------|---------|------|
| Global cache structure | `~/.jot/jdks/`, `~/.jot/jars/`, `~/.jot/resolve-cache/`. Content-addressable for JARs (SHA-256 → path). Hardlinks for deduplication (CoW if available via `reflink` crate) | 2 days |
| Cache cleanup | `jot clean --global` for cache pruning. Track last-used timestamps | 1 day |
| Integration tests | End-to-end: `jot init` → `jot java install 21` → verify JDK works → compile "Hello World" with raw `javac` invocation | 1 day |

**Deliverable:** `jot java install 21` downloads Adoptium JDK 21. `jot java list` shows it. Projects pin to a version via `jot.toml`.

**Manual platform test:** At end of Phase 1, manually test JDK download + extraction on Windows and macOS.

---

## Phase 2: Dependency Resolution (Weeks 6–12)

**Goal:** Resolve dependency trees from Maven Central. Generate `jot.lock`. This is the hardest technical phase — 7 weeks including a dedicated hardening week.

### Week 6–7: Maven POM Parser

| Task | Details | Est. |
|------|---------|------|
| POM XML parser | Parse `pom.xml` from Maven Central. Handle: `<dependencies>`, `<dependencyManagement>`, `<parent>` inheritance, `<properties>` interpolation (`${spring.version}` → value), `<exclusions>` | 5 days |
| Parent POM resolution | Recursive parent chain. Cache resolved POMs. Handle `<relativePath>` (skip — not relevant for Central) | 2 days |
| BOM import | `<scope>import</scope>` + `<type>pom</type>` in `<dependencyManagement>`. Flatten BOM dependency management into current resolution context. **BOM versions win over transitive versions** — implement as version override layer in resolver | 3 days |

**Critical detail on BOM handling:** When a BOM pins `jackson-databind` to `2.18.0` and a transitive dep declares `[2.17.0, 2.17.5]`, the BOM version (`2.18.0`) overrides. Implement by pre-populating version pins from BOM before running PubGrub — effectively treating BOM entries as top-level constraints.

### Week 8–9: PubGrub Integration

| Task | Details | Est. |
|------|---------|------|
| pubgrub-rs adapter | Implement `DependencyProvider` trait for Maven coordinates. Map Maven version ranges to PubGrub constraints. Handle `RELEASE`, `LATEST`, `[1.0,2.0)` range syntax | 4 days |
| Version ordering | Maven version comparison is not semver — it has qualifiers (`-SNAPSHOT`, `-alpha`, `-rc`), numeric segments, and a specific ordering spec. Port from Maven's `ComparableVersion` | 2 days |
| Optional dependencies | Maven optional deps are not transitive by default. Only include when explicitly re-declared by consumer | 1 day |
| Exclusions | `<exclusions>` on a dep remove transitives. When A depends on B (excluding C), and B depends on C, C is not resolved for A's subtree | 1 day |
| Classifier support | `<classifier>` (e.g., `sources`, `javadoc`, platform-specific JARs). Separate artifacts — same group:artifact:version but different files | 1 day |
| Relocations | `<relocation>` in POMs redirects old coordinates to new ones. Follow relocation chain | 1 day |

### Week 10: Maven Central Client

| Task | Details | Est. |
|------|---------|------|
| HTTP client | Parallel HTTP/2 downloads via `reqwest` + `tokio`. Connection pooling. Respect Maven Central rate limits | 2 days |
| POM fetching | Fetch from `https://repo1.maven.apache.org/maven2/`. Cache raw POM XML in `~/.jot/resolve-cache/` | 1 day |
| JAR downloading | Download resolved JARs to content-addressable cache. SHA-256 verify against Maven Central checksums | 1 day |
| Local repository fallback | Check `~/.m2/repository/` as a read-only fallback cache. Useful for offline work and existing Maven users | 1 day |

### Week 11: Lockfile & Dependency Commands

| Task | Details | Est. |
|------|---------|------|
| `jot.lock` generation | Deterministic format: sorted dependency list, SHA-256 for each JAR, resolution metadata. Single lockfile for workspace (all members resolved together). Regenerate on `jot add`/`jot remove`/`jot update` | 2 days |
| `jot add <dep>` | Interactive: search Maven Central, pick version, add to `jot.toml`, re-resolve, update lock | 1 day |
| `jot remove <dep>` | Remove from `jot.toml`, re-resolve, update lock | 0.5 day |
| `jot tree` | Print dependency tree (direct → transitive). Show version conflicts and resolution winners. `--workspace` flag for full workspace graph | 1 day |
| `jot lock` | Force-regenerate lockfile | 0.5 day |

### Week 12: Resolver Hardening

| Task | Details | Est. |
|------|---------|------|
| Compatibility test suite | Curate top 100 most-downloaded Maven Central packages. For each: resolve full tree with jot, compare against Maven's resolution output. Automate as CI job | 3 days |
| Demo dependency resolution | Resolve the shopflow workspace deps: Jackson, Helidon SE, picocli. Verify every transitive dep against `mvn dependency:tree` | 1 day |
| Edge case hardening | Fix failures from compatibility suite. Focus on: multi-level BOM chains, property interpolation edge cases, relocated artifacts (`javax` → `jakarta`) | 1 day |

**Deliverable:** Shopflow workspace dependency trees resolve correctly. Compatibility suite passes 95%+ of top 100 packages. `jot.lock` is deterministic.

---

## Phase 3: Compilation & Running (Weeks 13–17)

**Goal:** `jot build` compiles Java source → library JAR + fat-jar binary. `jot run` executes it. `jot test` runs JUnit 5. All three demo modules work as standalone projects first.

### Week 13: javac Orchestration

| Task | Details | Est. |
|------|---------|------|
| Source file discovery | Walk `src/main/java/` recursively. Collect `.java` files. Handle `source-dirs` override from config | 1 day |
| Classpath assembly | From resolved + downloaded deps, build `-classpath` argument. Use hardlinks from cache into `target/deps/` for locality | 1 day |
| javac invocation | Shell out to `javac` from managed JDK. Pass source files, classpath, `-d target/classes/`, `-source`/`-target` from toolchain version. Capture stderr for error reporting | 2 days |
| Error reformatting | Parse javac error output. Reformat with colors, file paths relative to project root | 1 day |

### Week 14: Annotation Processors & JAR Packaging

| Task | Details | Est. |
|------|---------|------|
| `[processors]` handling | Detect processors in config. Add to javac via `-processorpath`. Test with picocli-codegen (csvq module) | 2 days |
| Library JAR | Package `target/classes/` + `src/main/resources/` into `target/<name>-<version>.jar`. Generate `MANIFEST.MF` | 1 day |
| Fat-jar packaging | Unpack all dependency JARs, merge with project classes into single executable JAR. Handle `META-INF/services/` merging (SPI — concatenate provider files), duplicate-class detection + warning, `Main-Class` manifest entry, strip JAR signatures | 2 days |

**Fat-jar detail:** Since we're targeting Helidon SE (not Spring Boot), a standard shade-style fat-jar works perfectly. Unpack all dep JARs into a merged directory, copy project classes on top, concatenate `META-INF/services/` files for SPI, set `Main-Class` in manifest. No custom classloader needed.

### Week 15: Running & Testing

| Task | Details | Est. |
|------|---------|------|
| `jot run` | Invoke `java -cp ... <main-class>` with managed JDK. Pass `--` args to the app. Handle `--bin` selection for `[[bin]]` projects | 1 day |
| `jot build` orchestration | Full pipeline: resolve deps → download → compile → package. Show progress. Library JAR always, fat-jar only when `main-class` or `[[bin]]` declared | 1 day |
| `jot clean` | Delete `target/` directory | 0.5 day |
| Test compilation | Discover `src/test/java/`. Compile test sources with test-dependencies + main classes on classpath | 1 day |
| JUnit 5 runner | Download `junit-platform-console-standalone` JAR to cache. Invoke it with `--classpath ... --scan-classpath`. No custom test runner | 1 day |

### Week 16: Test Output & Single-Project Demos

| Task | Details | Est. |
|------|---------|------|
| Test output formatting | Parse JUnit console output. Show pass/fail summary with colors, durations. `--filter` for test name pattern | 1 day |
| Demo: domain standalone | Build `shopflow-domain` as standalone project (its own `jot.toml`, no workspace). Library JAR. Tests pass | 1 day |
| Demo: csvq standalone | Build `csvq` as standalone. `jot run -- --help` works. Fat-jar works: `java -jar target/bin/csvq` | 1 day |
| Demo: api standalone | Build `shopflow-api` as standalone (Jackson dep directly, no path dep on domain yet). `jot run` starts HTTP server | 1 day |

### Week 17: `jot init` Templates (Single-Project)

| Task | Details | Est. |
|------|---------|------|
| Template engine | Simple file-copy + variable substitution (`{{project-name}}`, `{{group}}`, `{{main-class}}`). Templates stored as embedded Rust resources | 1 day |
| `jot init --template java-lib` | Generates: `jot.toml` (no main-class), `src/main/java/` stub class, `src/test/java/` stub test, `.gitignore` | 1 day |
| `jot init --template java-cli` | Generates: `jot.toml` with `main-class` + picocli dep + processor, stub CLI main class, stub test | 1 day |
| `jot init --template java-server` | Generates: `jot.toml` with `main-class` + Helidon SE deps, stub server with `/health` route, stub test | 1 day |
| Interactive `jot init` | When run without `--template`, prompt for project name, type (lib/cli/server), group (optional). `dialoguer` crate | 1 day |

**Deliverable:** All three modules build, run, and pass tests as standalone projects. `jot init` scaffolds any of them from scratch.

**Manual platform test:** At end of Phase 3, manually test `jot build && jot run` for each demo on Windows and macOS.

---

## Phase 4: Workspaces (Weeks 18–20)

**Goal:** Full workspace support. The `shopflow` workspace builds all three modules with one command, with path deps, shared catalog, toolchain inheritance, and a single lockfile.

### Week 18: Workspace Discovery & Config

| Task | Details | Est. |
|------|---------|------|
| Workspace detection | Walk up from CWD to find workspace root (`jot.toml` with `[workspace]`). Parse `members` list. Discover member `jot.toml` files | 1 day |
| Config inheritance | Workspace root `[toolchains]` inherited by all members. Members override specific keys (e.g., `java = "8"` in legacy module). Unmentioned keys inherited | 2 days |
| `group` inheritance | Workspace-level `group` inherited by members. Members can override | 0.5 day |
| Shared catalog | Single `libs.versions.toml` at workspace root. All members resolve `catalog = "..."` against it | 1 day |
| Validation | Error on: member not found, circular path deps, duplicate module names, missing workspace root fields | 0.5 day |

### Week 19: Workspace Build DAG & Resolution

| Task | Details | Est. |
|------|---------|------|
| Path dependency resolution | `{ path = "../domain" }` resolves to the member's compiled JAR. Build the dependency graph between workspace members | 2 days |
| Build ordering | Topological sort of workspace members by path deps. `domain` builds before `api`. Members with no inter-deps (`csvq`) can build in parallel | 1 day |
| Unified resolution | All workspace members' external dependencies resolved together into a single `jot.lock`. Shared versions across the workspace — no version conflicts between members | 2 days |

### Week 20: Workspace Commands & Template

| Task | Details | Est. |
|------|---------|------|
| `jot build` (workspace) | Build all members in dependency order. Show per-module progress. Pass `--module <name>` to build a single member (+ its deps) | 1 day |
| `jot test` (workspace) | Run tests for all members. Show per-module results. `--module <name>` filter | 1 day |
| `jot run --module <name>` | Run a specific module's binary. Error if module has no main-class | 0.5 day |
| `jot tree --workspace` | Show full workspace dependency graph including inter-module deps | 0.5 day |
| `jot init --template java-workspace` | Generate the full `shopflow` workspace: root `jot.toml`, `libs.versions.toml`, three member modules with source stubs and tests | 1 day |
| End-to-end workspace demo | `jot init --template java-workspace shopflow && cd shopflow && jot build && jot test && jot run --module api` — full demo works | 1 day |

**Deliverable:** The `shopflow` workspace builds all three modules with dependency ordering. Path deps work. Shared catalog and toolchains work. Single lockfile. The "one command builds everything" demo is live.

---

## Phase 5: Formatting, Linting & Audit (Weeks 21–24)

**Goal:** `jot fmt`, `jot lint`, `jot audit` work out of the box, workspace-aware. Complete developer workflow.

### Week 21: Formatting

| Task | Details | Est. |
|------|---------|------|
| google-java-format integration | Download GJF JAR to cache. Invoke `java -jar google-java-format.jar` on source files. Handle `--check` mode (exit 1 on diff). Pin GJF version to jot release | 2 days |
| `[format]` config | Support `java-style = "google"` (default) vs `"aosp"` | 0.5 day |
| `jot fmt` command | Format all `.java` files. Show file count and changed files. `--check` for CI. In workspace: format all members, show per-module count | 1 day |
| Test on workspace | Run `jot fmt` on shopflow workspace, verify all three modules formatted | 0.5 day |

### Week 22: Linting

| Task | Details | Est. |
|------|---------|------|
| PMD integration | Download PMD distribution to cache. Invoke PMD CLI on source files. Parse XML output, reformat as terminal output with file:line references | 3 days |
| Default ruleset | Ship a curated PMD ruleset (subset of built-in rules). Embedded resource, write to temp file before invoking PMD | 1 day |
| `[lint]` config | Support `pmd-ruleset = "pmd-rules.xml"` for custom rules | 0.5 day |

### Week 23: Dependency Auditing & Resolver Expansion

| Task | Details | Est. |
|------|---------|------|
| OSV client | Query `https://api.osv.dev/v1/querybatch` with resolved dependency coordinates. Parse response for known CVEs | 2 days |
| `jot audit` output | Show severity, CVE ID, affected package, fixed version, transitive chain. Actionable fix command. In workspace: audit unified lockfile, show which members are affected | 1 day |
| `jot audit --fix` | Auto-bump direct deps to patched versions. Regenerate lockfile | 0.5 day |
| `jot audit --ci` | Exit 1 on critical/high severity | 0.5 day |

### Week 24: Resolver Compatibility Expansion

| Task | Details | Est. |
|------|---------|------|
| Expand test suite | Grow from top 100 to top 200 Maven Central packages. Add Helidon, Quarkus, Micronaut, Vert.x as specific test cases | 2 days |
| Fix failures | Address resolution failures. Focus on: nested BOM chains, relocated artifacts, unusual property interpolation | 2 days |
| Error message polish | For every unsupported POM feature, produce a clear error. Never a cryptic resolver crash | 1 day |

**Deliverable:** Complete development workflow. `jot fmt`, `jot lint`, `jot audit` work across the workspace. Resolver handles 95%+ of top 200 Maven Central packages.

---

## Phase 6: Polish & Launch (Weeks 25–27)

**Goal:** Production-grade quality. Documentation. Public launch.

### Week 25: Error Handling & Performance

| Task | Details | Est. |
|------|---------|------|
| Error message audit | Review every error path. Human-readable message, context (which file/dep/config), actionable suggestion | 2 days |
| Offline mode | Graceful degradation when offline. Use cached JDKs, cached JARs, cached resolution | 1 day |
| Concurrent jot invocations | File locking on `jot.lock` and cache writes. Prevent corruption from parallel `jot build` in CI | 1 day |
| Performance benchmarking | Measure cold-start and warm-start on workspace. Target: warm workspace build under 10 seconds. Ensure parallel JAR downloads saturate bandwidth | 1 day |

### Week 26: Documentation & Website

| Task | Details | Est. |
|------|---------|------|
| README | Quick start, comparison table (jot vs Maven vs Gradle), command reference | 1 day |
| User guide | Getting started, `jot.toml` reference, `libs.versions.toml` reference, workspace guide, template walkthroughs, FAQ | 2 days |
| Website | `jot.dev` — landing page, install instructions, docs. Static site (Astro or similar) | 2 days |

### Week 27: Launch

| Task | Details | Est. |
|------|---------|------|
| Release v0.1.0 | Tag, build, upload binaries. Homebrew formula, scoop manifest, AUR package | 2 days |
| Launch post | Blog post / HN / Reddit / Twitter. Demo video: workspace init → build → test → run → fmt → lint → audit in 90 seconds | 1 day |
| Feedback triage | Set up GitHub Issues templates. Prioritize: resolution bugs > workspace bugs > compilation bugs > UX | 2 days |

**Manual platform test:** Full end-to-end workspace demo on Windows and macOS before release.

**Deliverable:** Public v0.1.0 with binaries for Linux, macOS (Intel + ARM), and Windows.

---

## Post-Launch Prioritization

| Priority | Feature | Why |
|----------|---------|-----|
| **P0** | Bug fixes from real users | Trust and reliability |
| **P1** | Spring Boot support | Most common Java project type — needs custom repackaging |
| **P1** | `native-image` dist | CLI tool developers want this immediately |
| **P1** | Kotlin support | Opens jot to the other half of the JVM ecosystem |
| **P2** | `jot doctor` (static) | High-value, moderate effort |
| **P2** | `test-support` sharing | `include = "test-support"` for shared test fixtures |
| **P3** | Plugin system | Unblocks community contributions |
| **P3** | Observability commands | Differentiator, but not blocking adoption |
| **P3** | `jot publish` | Only needed by library authors |
| **P4** | `jot bench` | Nice-to-have |
| **P4** | `jot tool run/install` | `npx` equivalent |

---

## Risk Mitigation Strategies

### Risk 1: Maven POM Long Tail (Severity: Critical)

**Mitigation:**
- Compatibility test suite from day one (top 200 packages)
- Two dedicated hardening weeks (Week 12 and Week 24)
- Clear errors for unsupported POM features, never cryptic crashes
- Target 95%, iterate on the remaining 5% based on real user reports

### Risk 2: Workspace Build Ordering (Severity: Medium)

**Scenario:** Circular path deps, incorrect topological sort, stale JARs from previous builds polluting classpath.

**Mitigation:**
- Detect circular deps at config parse time (Phase 4, Week 18)
- Always clean-build path dep JARs before dependents (no stale artifact risk)
- Start simple: sequential build in topo order. Parallel builds for independent members is an optimization, not a requirement

### Risk 3: Fat-JAR Edge Cases (Severity: Medium)

**Mitigation:**
- Test on Helidon SE (heavy SPI usage)
- Log duplicate classes as warnings
- Strip JAR signatures during merge (standard practice)

### Risk 4: Windows Parity (Severity: High)

**Mitigation:**
- Cross-compile Windows binaries in Linux CI from day one
- Manual test on Windows at each phase boundary (5 manual test sessions over 27 weeks)
- Use `std::path::PathBuf` everywhere
- `fs2` crate for cross-platform file locking
- Hardlink fallback to copies on Windows

### Risk 5: Solo Burnout (Severity: Medium)

**Mitigation:**
- Clear deliverables every 2–3 weeks
- Phase 3 (Week 16) produces the first "wow" moment
- Phase 4 (Week 20) produces the workspace demo — second "wow" moment
- Dev logs or streaming for community engagement

---

## Testing Strategy

| Layer | What | Tool | When |
|-------|------|------|------|
| Unit | Config parsing, version comparison, POM parsing, lockfile generation, workspace graph | `cargo test` | Every commit (Linux) |
| Integration | Full resolve → download → compile → run cycles | Custom test harness (temp dirs, real Maven Central) | Every PR (Linux) |
| Compatibility | Top 200 Maven Central POMs resolve correctly | Automated suite comparing against Maven's resolution | Nightly (Linux) |
| End-to-end | Workspace demo: init → build → test → run → fmt → lint → audit | CI job that runs full demo | Every release (Linux) |
| Platform | Windows + macOS manual verification | Manual testing | End of each phase (~5 sessions) |

---

## Success Criteria for v0.1

1. **Workspace demo works end-to-end** — `jot init --template java-workspace → jot build → jot test → jot run --module api` verified on Linux (CI) and manually on Windows + macOS
2. **Build ordering is correct** — `domain` compiles before `api`, `csvq` builds independently
3. **Single lockfile** — workspace resolves all external deps together, deterministic across machines
4. **Path deps work** — `api` module consumes `domain` library JAR via `{ path = "../domain" }`
5. **Toolchain inheritance works** — workspace root sets `java = "21"`, members inherit unless overridden
6. **Shared catalog works** — `libs.versions.toml` at workspace root, all members reference it
7. **Zero runtime classpath errors** — no `ClassNotFoundException`, no `NoSuchMethodError`
8. **`jot fmt`, `jot lint`, `jot audit` are workspace-aware** — operate across all members
9. **Error messages are helpful** — every failure includes what, why, and what to do
10. **Resolver passes 95%+ of top 200 Maven Central packages**

---

## Timeline Summary

| Weeks | Phase | Key Milestone |
|-------|-------|--------------|
| 1–2 | Skeleton & CI | `jot --version`, config parser, Linux CI + cross-compiled binaries |
| 3–5 | JDK Management | `jot java install 21` works |
| 6–12 | Dependency Resolution | All demo trees resolve. 95% compat suite. `jot.lock` works |
| 13–17 | Compilation & Running | All three demos: build, run, test as standalone. `jot init` templates |
| 18–20 | Workspaces | Full workspace: build DAG, path deps, shared catalog, toolchain inheritance |
| 21–24 | Fmt, Lint, Audit | Complete workspace-aware dev workflow. Resolver hardened to top 200 |
| 25–27 | Polish & Launch | v0.1.0 public release |

**Total: ~7 months to production-grade v0.1 with full workspace support.**