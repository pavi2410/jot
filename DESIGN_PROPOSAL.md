# `jot` — The Missing Tool for the JVM

> A unified, Rust-powered toolchain for Java and Kotlin that does for the JVM what `uv` did for Python and `bun` did for JavaScript.

**Author:** Pavitra  
**Status:** Serious Exploration — v12  
**Date:** March 2026

---

## 1. The Thesis

Every mature language ecosystem eventually hits the same inflection point: the toolchain fragments into a dozen specialized utilities that don't talk to each other. A single, fast, opinionated tool then arrives and collapses the stack. Python had `uv`. JavaScript had `bun`. The JVM is overdue.

| Concern | Current tools | Pain |
|---|---|---|
| JDK version management | SDKMAN, Jabba, manual downloads | Shell hacks, no per-project pinning |
| Build & compilation | Maven, Gradle | Maven: XML-verbose, slow. Gradle: fast but complex, opaque DSL |
| Dependency resolution | Built into Maven/Gradle | Slow cold-start, heavyweight daemons, poor caching |
| Project scaffolding | Maven archetypes, `gradle init`, Spring Initializr | Heavyweight, framework-opinionated |
| Tool/CLI execution | Manual JAR management | No `npx`/`uvx` equivalent |
| JVM observability | javap, jcmd, JFR, jmap, jstack, async-profiler, MAT | Scattered tools, arcane flags, heavy GUIs |

**`jot`** unifies all of this into a single, fast, native binary.

---

## 2. Landscape Analysis

### JBang
Single-file Java/Kotlin scripting with inline `//DEPS`. Not a build tool. jot fills the project-management gap.

### Maven Daemon (mvnd)
~40% faster Maven builds via daemon/warm JVM. Same XML, same plugins, no lockfile, no JDK management.

### Mill
3-7x faster builds via caching/parallelism. No JDK management, no tool-execution, minimal Java adoption.

### Amper (JetBrains)
Experimental Kotlin/JVM build tool. jot borrows: scope-as-annotation (`exported`), toolchain catalogs, batteries-included philosophy.

### Declarative Gradle
Experimental declarative layer. Key insight: domain concepts over build-tool abstractions.

### Coursier
Fast Scala dependency resolver. Proves fast Maven Central resolution is achievable.

---

## 3. Design Principles

### 3.1 One Binary, Zero Dependencies
Single static Rust binary. Downloads and manages JDKs as a first-class feature.

### 3.2 Obsessive Performance
Rust-native PubGrub resolver, parallel HTTP/2, content-addressable global cache with hardlinks/CoW.

### 3.3 Interop, Not Drop-In Replacement
Own `jot.toml`, resolves from Maven Central. `jot import` for migration. Publishes in standard format.

### 3.4 Progressive Disclosure
```bash
curl -fsSL https://jot.dev/install.sh | sh
jot init myapp && cd myapp && jot run
```

### 3.5 Correctness via Lockfiles
`jot.lock` — deterministic, SHA-256 verified.

### 3.6 The Classpath Is Dead (to the user)
jot assembles classpaths. `jot doctor` provides human-readable diagnostics.

### 3.7 Java Is a Compiled Language
Library + multiple named binaries from same codebase. `[[bin]]` mirrors Cargo.

### 3.8 Opinionated Formatting & Linting
Built-in like `rustfmt`/`clippy`. Versions pinned to jot release. Long-term: native Rust implementations.

### 3.9 JVM Observability Without Leaving the Terminal
The JVM has world-class introspection capabilities (JFR, jcmd, jmap, jstack) but they're buried behind arcane flags and heavy GUI tools. jot surfaces them as simple subcommands with terminal-first output.

---

## 4. Toolchains

`[toolchains]` is the single source of truth for compiler and runtime versions. Declaring a toolchain implicitly defines language support and activates the corresponding `$name` catalog.

### 4.1 Syntax

```toml
[toolchains]
java = "21"                                         # always required
java = { version = "21", vendor = "corretto" }      # with vendor override
kotlin = "2.1.0"                                     # activates $kotlin catalog
graalvm = { version = "21", variant = "ce" }         # for native-image
# scala = "3.5.0"                                    # future
```

`java` is always required. `kotlin` must be explicit with a version — jot does not ship a default Kotlin version.

### 4.2 Language Inference

No `lang` field. Language support is inferred from `[toolchains]` + source files:

| `[toolchains]` | Source files | Behavior |
|---|---|---|
| `java` only | `.java` | javac only |
| `java` + `kotlin` | `.java` only | javac only |
| `java` + `kotlin` | `.kt` only | kotlinc only |
| `java` + `kotlin` | `.java` + `.kt` | Two-pass: kotlinc → javac |
| `java` only | `.kt` found | **Error:** "Found .kt files but no Kotlin toolchain declared" |

### 4.3 Workspace Default + Per-Module Override

Workspace root sets defaults. Modules override specific keys. Unmentioned keys inherited.

```toml
# Workspace root
[toolchains]
java = "21"
kotlin = "2.1.0"
```

```toml
# legacy-client/jot.toml
[toolchains]
java = "8"       # overrides java; kotlin inherited but unused (no .kt files)
```

### 4.4 Toolchain Catalogs

Declaring a recognized toolchain activates `$name` catalog:

```toml
[toolchains]
kotlin = "2.1.0"
```
```toml
[dependencies]
coroutines = { catalog = "$kotlin.coroutines-core" }
```

Future catalogs: `$compose`, `$spring`, `$quarkus`, `$micronaut`.

### 4.5 JDK Vendors

| Vendor | Key | Notes |
|---|---|---|
| Eclipse Adoptium | `adoptium` (default) | Community-backed |
| Amazon Corretto | `corretto` | AWS-optimized |
| GraalVM | `graalvm` | Via separate `graalvm` entry |
| Oracle OpenJDK | `oracle` | Official Oracle builds |
| Azul Zulu | `zulu` | Wide platform coverage |

---

## 5. Formatting & Linting

Built-in and opinionated. Versions pinned to jot release.

```bash
$ jot fmt              # format all source files
$ jot fmt --check      # check only (CI mode)
$ jot lint             # lint all source files
$ jot lint --fix       # auto-fix where possible
```

| Command | Java files | Kotlin files |
|---|---|---|
| `jot fmt` | google-java-format (bundled) | ktfmt (bundled) |
| `jot lint` | PMD (bundled, standalone CLI) | detekt (bundled) |

PMD is used for Java linting instead of error-prone because PMD is a standalone source-file analyzer with 400+ built-in rules, while error-prone is a javac compiler plugin that would require a separate recompilation pass.

**Minimal configuration overrides:**

```toml
[format]
java-style = "google"             # default; or "aosp" for Android

[lint]
detekt-config = ".detekt.yml"     # optional custom detekt rules
pmd-ruleset = "pmd-rules.xml"     # optional custom PMD ruleset
```

These sections are entirely optional. Without them, jot uses sensible defaults.

**Long-term vision:** Native Rust formatter and linter for 100x+ speedup.

---

## 6. Benchmarking

`jot bench` wraps JMH (Java Microbenchmark Harness) with zero setup. JMH is the gold standard for JVM benchmarking — developed by the same people who build the JVM's JIT compiler — but setting it up today requires annotation processor config, a separate source set, and a build plugin. Most developers skip benchmarking entirely because the friction is too high.

**Convention:** Benchmark classes live in `src/bench/java/` (or `src/bench/kotlin/`). jot auto-adds JMH dependencies — the user never declares them. Classes with `@Benchmark` methods are auto-detected.

```java
// src/bench/java/com/shopflow/orders/OrderSerializerBench.java
import org.openjdk.jmh.annotations.*;

@State(Scope.Benchmark)
public class OrderSerializerBench {
    private Order order;
    private ObjectMapper mapper;

    @Setup
    public void setup() {
        order = new Order("ORD-123", BigDecimal.valueOf(99.99));
        mapper = new ObjectMapper();
    }

    @Benchmark
    public String toJson() throws Exception {
        return mapper.writeValueAsString(order);
    }

    @Benchmark
    public Order fromJson() throws Exception {
        return mapper.readValue("{\"id\":\"ORD-123\",\"amount\":99.99}", Order.class);
    }
}
```

```bash
$ jot bench
  → Compiling 2 benchmark classes...
  → Running benchmarks (fork: 1, warmup: 3 iter, measurement: 5 iter)

  Benchmark                          Mode  Cnt    Score   Error  Units
  OrderSerializerBench.toJson        avgt    5   42.3 ±  1.2    ns/op
  OrderSerializerBench.fromJson      avgt    5  187.6 ±  4.8    ns/op

  → Results saved: target/bench/2026-03-21T14:30:00.json

$ jot bench --filter "toJson"        # run subset
$ jot bench --compare                # diff against last saved run
  toJson:   42.3 ns/op → 38.1 ns/op  (-9.9%) ✓ faster
  fromJson: 187.6 ns/op → 192.1 ns/op (+2.4%) ~ within noise
```

Opinionated defaults, no configuration. jot picks sane JMH parameters (1 fork, 3 warmup, 5 measurement iterations). Results are persisted in `target/bench/` and `--compare` diffs against the most recent previous run.

---

## 7. Dependency Auditing

`jot audit` scans all dependencies against the OSV (Open Source Vulnerabilities) database for known CVEs. Zero config — no plugin, no API key.

```bash
$ jot audit
  → Checking 47 dependencies against OSV database...

  CRITICAL  CVE-2024-38816  spring-webmvc 6.1.6
            Remote code execution via crafted request
            Fixed in: 6.1.14
            → jot update spring-boot  (BOM will pull fixed version)

  MODERATE  CVE-2024-7254   protobuf-java 3.25.1
            Stack overflow via deeply nested message
            Fixed in: 3.25.5
            Transitive via: grpc-protobuf → protobuf-java
            → jot add protobuf-java --version 3.25.5  (override transitive)

  45 dependencies clean.

$ jot audit --fix          # auto-bump to patched versions
  → Updated spring-boot: 3.4.0 → 3.4.1 (fixes CVE-2024-38816)
  → Added override: protobuf-java 3.25.5 (fixes CVE-2024-7254)
  → Regenerated jot.lock

$ jot audit --ci           # exit 1 on critical/high vulns (for CI gates)
```

`jot audit` queries the OSV database (same source used by `cargo audit` and `npm audit`), matches against the resolved dependency graph in `jot.lock`, and provides actionable fix suggestions with the exact jot commands to run. `--fix` applies patches automatically. `--ci` is a non-zero exit code mode for CI pipeline gating.

---

## 8. JVM Observability

The JVM has world-class runtime introspection — JFR (Java Flight Recorder), `jcmd`, `jmap`, `jstack`, async-profiler — but using them requires remembering arcane flags, finding PIDs, parsing verbose output, and context-switching to heavy GUI tools like VisualVM, JDK Mission Control, or Eclipse MAT. jot wraps these existing JDK capabilities with sane defaults and terminal-first output.

### 6.1 `jot inspect` — Bytecode & Class Analysis

Examine compiled classes without leaving the terminal. Wraps `javap` and embeds a decompiler.

```bash
# Bytecode disassembly
$ jot inspect com.shopflow.orders.Order
  class com.shopflow.orders.Order
  ├── compiled with: javac 21 (classfile 65.0)
  ├── size: 2.4 KB
  ├── fields: 4
  ├── methods: 12 (3 synthetic)
  │
  │ public java.lang.String getName()
  │   0: aload_0
  │   1: getfield      #2  // Field name:Ljava/lang/String;
  │   4: areturn

# Decompile back to Java source
$ jot inspect --decompile com.shopflow.orders.Order
  // Decompiled from Order.class
  public class Order {
      private final String name;
      private final BigDecimal amount;
      ...
  }

# Class size breakdown — useful for native-image and JAR size optimization
$ jot inspect --size com.shopflow.orders.Order
  Total: 2,412 bytes
  ├── constant pool: 1,204 bytes (49.9%)
  ├── methods: 847 bytes (35.1%)
  ├── fields: 196 bytes (8.1%)
  └── attributes: 165 bytes (6.8%)

# What does this class depend on?
$ jot inspect --deps com.shopflow.orders.Order
  → java.math.BigDecimal
  → java.time.Instant
  → com.shopflow.domain.Customer
  → com.fasterxml.jackson.annotation.JsonProperty
```

Use cases: debugging Lombok/annotation-processor generated code, verifying Kotlin data class bytecode, checking classfile compatibility level, understanding dependency chains at the class level.

### 6.2 `jot doctor` — Static & Runtime Diagnostics

`jot doctor` (already in the proposal) is extended with runtime classpath analysis:

```bash
# Static analysis (existing) — dependency version conflicts
$ jot doctor
  → Checking dependency graph... ok
  → Checking toolchain versions... ok
  → Checking lockfile integrity... ok

# Runtime classpath analysis — the killer feature
$ jot doctor --runtime [--bin server]
  → Scanning classpath of order-service/server...

  ⚠ Duplicate classes detected:
    com.fasterxml.jackson.databind.ObjectMapper
      ├── jackson-databind-2.18.0.jar (from spring-web)
      └── jackson-databind-2.17.3.jar (from legacy-client)
      → 2.18.0 wins (nearest-first). May cause NoSuchMethodError.
         Fix: add exclusion on legacy-client dependency.

  ⚠ Split package detected:
    javax.annotation (in both jsr305-3.0.2.jar and jakarta.annotation-api-2.1.1.jar)
    → May cause issues with JPMS. Consider excluding jsr305.

  ✓ No shadowed classes.
  ✓ No circular class dependencies.
```

This catches the class of bugs — `ClassNotFoundException`, `NoSuchMethodError`, `LinkageError` — that costs teams days of debugging. jot detects them statically before the app even runs.

### 6.3 `jot profile` — Runtime Profiling

Wraps JFR, GC logging, and heap analysis behind simple subcommands. All profiling uses existing JDK technology — jot adds UX, not new instrumentation.

**CPU profiling** — "Which method is hot?"

```bash
$ jot profile cpu [--bin server]
  → JFR recording started (low-overhead, production-safe)
  → Press Ctrl+C to stop and analyze...
  ^C
  → Top hot methods (10s sample):
    38.2%  c.s.orders.OrderRepository.findByStatus()
    12.1%  c.fasterxml.jackson.databind.ObjectMapper.writeValueAsString()
     8.4%  org.hibernate.SQL.execute()
     6.7%  java.util.stream.ReferencePipeline.collect()
     4.2%  [GC]
  → Flame graph: target/profile/cpu-flamegraph.html
```

**Heap analysis** — "Why is my app eating 2GB?"

```bash
$ jot profile heap [--bin server]
  → Connected to PID 48291 (order-service/server)
  → Top retained objects:
    1. byte[]                    482 MB  (38%)  ← likely cache or buffer
    2. java.lang.String          189 MB  (15%)
    3. c.s.orders.OrderEntity[]  156 MB  (12%)  ← 847,000 instances
    4. java.util.HashMap$Node     98 MB   (8%)
  → Suggestion: OrderEntity[] dominance suggests unbounded query result.
     Consider pagination or streaming.
  → Full heap dump: target/profile/heap.hprof (open in MAT for deep dive)
```

Not a full MAT replacement — a quick triage tool that answers "what's eating memory?" in 10 seconds.

**GC analysis** — "Why is my app pausing?"

```bash
$ jot profile gc [--bin server]
  → GC logging enabled, sampling for 30s...
  → GC Summary:
    Collector: G1 (JDK 21 default)
    Total pauses: 847ms over 30s (2.8% of wall time)
    Longest pause: 42ms (G1 mixed collection)
    Avg pause: 3.1ms
    Heap: 256MB used / 512MB committed / 2GB max
    Allocation rate: 180 MB/s
  → Suggestion: Allocation rate is high. Check for unnecessary object
     creation in hot paths (see `jot profile cpu`).
```

**Thread analysis** — "Is my app deadlocked?"

```bash
$ jot profile threads [--bin server]
  → Thread dump from PID 48291:
    Total: 47 threads
    RUNNABLE:     12 (25.5%)
    WAITING:      28 (59.6%)
    TIMED_WAITING: 5 (10.6%)
    BLOCKED:       2 (4.3%)  ← potential contention

  ⚠ Possible deadlock detected:
    Thread "order-processor-3" waiting on lock held by "order-processor-7"
    Thread "order-processor-7" waiting on lock held by "order-processor-3"
    → Both in: c.s.orders.sync.OrderLockManager.acquireLock()
```

### 6.4 Design Philosophy

All observability commands follow these principles:

**Terminal-first.** The primary output is a concise, readable summary in the terminal. Detailed artifacts (flame graphs, heap dumps, JFR recordings) are saved to `target/profile/` for deep dives with specialized tools.

**Wrapping, not reinventing.** jot uses existing JDK technology (JFR, `jcmd`, `jmap`, `jstack`, GC logs). No custom agents, no bytecode instrumentation, no overhead beyond what the JDK already provides. jot adds UX — sane defaults, human-readable output, actionable suggestions.

**Production-safe defaults.** CPU profiling uses JFR's low-overhead mode (<2% impact). Heap analysis uses live object histograms (no full dump by default). GC analysis parses logs without adding pause time. These are safe to run on staging environments.

**Actionable output.** Where possible, jot doesn't just show data — it suggests fixes. "Allocation rate is high → check hot paths." "Duplicate classes → add exclusion." This is the `jot doctor` philosophy extended to runtime.

---

## 9. Project Model

### 6.1 Library + Binaries (Cargo-style)

Every project implicitly produces a library JAR. Binaries are declared with `[[bin]]`.

**Single binary (simple form):**
```toml
[project]
name = "my-app"
group = "com.example"              # optional — required only for publishing
version = "1.0.0"
main-class = "com.example.Main"
dist = "fat-jar"                   # optional — default is fat-jar

[toolchains]
java = "21"
```

`main-class` + `dist` at project level is sugar for a single `[[bin]]`.

**Multiple binaries:**
```toml
[project]
name = "order-service"
group = "com.shopflow"
version = "1.0.0"

[toolchains]
java = "21"
graalvm = { version = "21", variant = "ce" }

[[bin]]
name = "server"
main-class = "com.shopflow.orders.OrderServiceApplication"

[[bin]]
name = "migrate"
main-class = "com.shopflow.orders.cli.MigrateCommand"
dist = "native-image"

[[bin]]
name = "seed"
main-class = "com.shopflow.orders.cli.SeedData"
```

**Pure library (no binaries):**
```toml
[project]
name = "shopflow-domain"
version = "1.0.0"

[toolchains]
java = "21"
```

**Build output:**
```bash
# Pure library
$ jot build
  → target/shopflow-domain-1.0.0.jar     (library JAR)

# Single binary
$ jot build
  → target/my-app-1.0.0.jar              (library JAR)
  → target/bin/my-app                     (fat-jar executable)

# Multiple binaries
$ jot build
  → target/order-service-1.0.0.jar        (library JAR)
  → target/bin/server                      (fat-jar)
  → target/bin/migrate                     (native binary)
  → target/bin/seed                        (fat-jar)
```

**Running:**
```bash
$ jot run                    # first [[bin]]
$ jot run --bin migrate      # specific binary
$ jot run --bin seed -- --count 1000
```

### 6.2 Distribution Formats

| Format | Output | Use case |
|---|---|---|
| `fat-jar` (default) | Single executable JAR | Server deployment |
| `native-image` | Platform-native binary (requires `graalvm` in toolchains) | CLI tools, serverless |
| `jlink` | Custom JRE + app bundle | Desktop apps |

### 6.3 Source Directory Convention

Default layout follows the Maven/Gradle standard:

```
src/
├── main/
│   ├── java/           # Java sources
│   ├── kotlin/         # Kotlin sources (if kotlin in toolchains)
│   └── resources/      # Resource files
├── test/
│   ├── java/
│   ├── kotlin/
│   └── resources/
└── test-support/
    └── java/           # Shared test utilities (for workspace `include = "test-support"`)
```

Override with `source-dirs` if needed:
```toml
[project]
source-dirs = ["src/main/kotlin", "src/main/java"]
test-dirs = ["src/test/kotlin"]
```

### 6.4 Publishing Multi-Binary Packages

When `jot publish` publishes a multi-binary JAR, binary metadata is embedded in `META-INF/jot-binaries.json`:

```json
{
  "binaries": [
    { "name": "server", "main-class": "com.shopflow.orders.OrderServiceApplication" },
    { "name": "migrate", "main-class": "com.shopflow.orders.cli.MigrateCommand" },
    { "name": "seed", "main-class": "com.shopflow.orders.cli.SeedData" }
  ]
}
```

Consumers select a binary:
```bash
$ jot tool run com.shopflow:order-service               # runs "server" (first binary)
$ jot tool run com.shopflow:order-service --bin migrate  # runs "migrate"
```

### 6.5 The `group` Field

`group` is optional. Required only for publishing to Maven Central (`jot publish` errors without it). In a workspace, `group` can be declared at the workspace level and inherited:

```toml
# Workspace root
[workspace]
members = ["domain", "order-service"]
group = "com.shopflow"           # inherited by all members

# domain/jot.toml — inherits group
[project]
name = "shopflow-domain"
version = "1.0.0"

# order-service/jot.toml — can override
[project]
name = "shopflow-order-service"
version = "1.0.0"
group = "com.shopflow.orders"   # override if needed
```

---

## 10. Dependency Model

### 10.1 Scopes

```toml
[dependencies]
spring-web = { catalog = "spring-web" }                                          # default: compile + runtime
jackson = { catalog = "jackson-core", exported = true }                          # visible to consumers
postgres = { coords = "org.postgresql:postgresql:42.7.0", scope = "runtime" }    # runtime only
servlet-api = { coords = "jakarta.servlet:jakarta.servlet-api:6.1.0", scope = "compile-only" }

[test-dependencies]
junit = { catalog = "junit" }

[processors]
lombok = "org.projectlombok:lombok:1.18.34"
```

### 10.2 Catalogs

**Gradle-compatible `libs.versions.toml`** at project or workspace root. Works for both standalone projects and workspaces — any project with a `libs.versions.toml` next to its `jot.toml` can use `{ catalog = "..." }` references.

```toml
[versions]
spring-boot = "3.4.0"
junit = "5.11.0"

[libraries]
spring-web = { module = "org.springframework.boot:spring-boot-starter-web", version.ref = "spring-boot" }
spring-jpa = { module = "org.springframework.boot:spring-boot-starter-data-jpa", version.ref = "spring-boot" }
spring-test = { module = "org.springframework.boot:spring-boot-starter-test", version.ref = "spring-boot" }
spring-bom = { module = "org.springframework.boot:spring-boot-dependencies", version.ref = "spring-boot" }
junit = { module = "org.junit.jupiter:junit-jupiter", version.ref = "junit" }

[bundles]
testing = ["junit"]
```

**BOM handling:** BOMs are declared as regular entries in `[libraries]` and referenced with a `bom = true` flag in `jot.toml`:

```toml
[dependencies]
spring-bom = { catalog = "spring-bom", bom = true }   # imported as BOM, not a regular dep
spring-web = { catalog = "spring-web" }                 # version resolved from BOM
```

This keeps the catalog file strictly Gradle-compatible (no custom `[boms]` section).

**Resolution order:** `$toolchain` catalogs → project `libs.versions.toml` → inline `coords = "..."`.

---

## 11. Plugin System

jot is designed plugin-first. Its own JUnit runner, formatting, linting, and annotation processor handling are implemented as internal plugins using the same API available to external plugin authors. This dogfooding ensures the API is powerful enough for real use cases.

### 11.1 Core Abstraction

A plugin is a JVM class that implements `JotPlugin`:

```java
public interface JotPlugin {
    String name();
    void setup(JotPluginContext ctx);
}
```

Plugins register **hooks** (lifecycle events) and **tasks** (input/output code generation) via the context object. Plugins cannot register custom CLI subcommands — the CLI namespace belongs to jot. Plugins that need a CLI ship their own binary or use `jot tool run`.

### 11.2 Hooks — Lifecycle Events (90% of Plugins)

Hooks are simple event handlers that react to build lifecycle stages. Multiple plugins can register for the same hook — they execute in declaration order from `jot.toml`, each receiving the output of the previous (chaining).

```java
public class SpringBootPlugin implements JotPlugin {
    public String name() { return "spring-boot"; }
    
    public void setup(JotPluginContext ctx) {
        // Add JVM args before running
        ctx.hooks().beforeRun(event -> {
            event.addJvmArg("-Dspring.devtools.restart.enabled=true");
        });
        
        // Repackage JAR after packaging
        ctx.hooks().afterPackage(event -> {
            springBootRepackage(event.outputJar(), event.dependencies());
        });
    }
}
```

**Hook points:**

```
configLoaded          After jot.toml is parsed, before anything else
dependenciesResolved  After dependency graph is resolved
beforeCompile         Source preprocessing, additional javac/kotlinc args
afterCompile          Bytecode postprocessing, instrumentation
beforeTest            Test environment setup, agent attachment
afterTest             Coverage reports, cleanup
beforePackage         Modify what goes into the JAR
afterPackage          Repackage, sign, transform artifacts
beforeRun             JVM arg injection, agent attachment
beforePublish         Validation, metadata enrichment
```

10 hook points. Compare to Maven's 23 lifecycle phases. Each hook receives a typed event object with relevant context and mutation methods.

**Properties:**
- **Stateless per invocation.** Each hook call receives an immutable snapshot plus mutation methods. No shared mutable state across plugins.
- **Chaining.** Multiple plugins on the same hook execute in declaration order. Each sees the accumulated mutations from previous plugins.
- **Fail-fast.** If a hook throws, the build stops with a clear error identifying the plugin and hook.

### 11.3 Tasks — Code Generation (10% of Plugins)

Tasks are for plugins that produce files from inputs: protobuf compilation, OpenAPI client generation, JOOQ codegen, Minecraft deobfuscation. Tasks declare inputs and outputs, enabling automatic dependency tracking, caching, and build ordering.

```java
public class ProtobufPlugin implements JotPlugin {
    public String name() { return "protobuf"; }
    
    public void setup(JotPluginContext ctx) {
        ctx.task("generate-protobuf")
            .inputs("src/main/proto/**/*.proto")
            .outputDir("generated/sources/proto")
            .action(task -> {
                ctx.exec("protoc", 
                    "--java_out=" + task.outputDir(), 
                    task.inputFiles());
            });
    }
}
```

**Automatic wiring:** jot inspects task outputs and wires them into the build lifecycle automatically:
- Output dir contains `.java`/`.kt` files → task runs before compilation, output added to source paths
- Output dir contains resource files → task runs before packaging, output added to resources
- No manual phase binding needed

**Incremental by default:** Tasks are cached by hashing their inputs. If proto files haven't changed, protoc doesn't run. This is the Gradle/Amper insight applied with minimal API surface.

**Parallel where possible:** Tasks with non-overlapping inputs/outputs can run in parallel. jot builds a DAG from declared inputs/outputs and parallelizes independent branches.

### 11.4 Plugin Declaration in `jot.toml`

```toml
[plugins]
# From Maven Central (resolved like any dependency)
protobuf = { coords = "dev.jot:jot-plugin-protobuf:1.0.0" }

# With configuration
openapi = { coords = "dev.jot:jot-plugin-openapi:2.3.0", config = { spec = "api.yaml", package = "com.example.api" } }

# From toolchain catalog
spring-devtools = { catalog = "$spring.devtools-plugin" }

# From local path (for plugin development)
my-plugin = { path = "../my-jot-plugin" }
```

Plugin configuration is inline TOML passed to the plugin's `setup()` method as a typed map. No external config files. Version-controlled alongside the build config.

### 11.5 Plugin Hosting Architecture

Plugins are JVM code running in a managed JVM process, communicating with jot's Rust core via IPC:

```
┌───────────────────────────┐     IPC (JSON over pipe)     ┌──────────────────────────┐
│      jot CLI (Rust)        │ ◄──────────────────────────► │    Plugin Host (JVM)      │
│                            │                              │                           │
│  • Config parsing          │   Hook invocations ──────►   │  • Internal plugins:      │
│  • Dependency resolution   │                              │    - junit-runner          │
│  • File watching           │   ◄────── Hook responses     │    - fmt (gjf, ktfmt)     │
│  • Build orchestration     │                              │    - lint (PMD, detekt)   │
│  • CLI interface           │   Task scheduling ───────►   │    - processor-handler    │
│  • Cache management        │                              │                           │
│                            │   ◄────── Task outputs       │  • External plugins:      │
│                            │                              │    - protobuf             │
│                            │                              │    - openapi              │
│                            │                              │    - spring-devtools      │
└───────────────────────────┘                              └──────────────────────────┘
```

**Why IPC, not in-process?** Inspired by Oxc's architecture research: the JVM can't share memory with Rust the way JS can via WASM. But our data structures (file paths, config maps, dependency lists) are far simpler than ASTs — serialization overhead is negligible. IPC also gives us isolation: a buggy plugin can't crash jot itself.

**Warm JVM.** The plugin host JVM stays alive between builds, caching classloaders and plugin instances. First build pays JVM startup (~500ms). Subsequent builds communicate with the warm host instantly. This mirrors mvnd's daemon insight.

**Plugin classpath isolation.** Each plugin gets its own classloader, preventing dependency conflicts between plugins. Plugin A using Jackson 2.17 and Plugin B using Jackson 2.18 won't collide.

### 11.6 Internal Plugins (Dogfooding)

jot's built-in features are implemented as internal plugins:

| Feature | Internal plugin | Hooks/tasks used |
|---|---|---|
| JUnit 5 runner | `jot-junit` | `beforeTest` (discover tests), custom test execution |
| Formatting | `jot-fmt` | `beforeCompile` (optional auto-format on build) |
| Linting | `jot-lint` | `afterCompile` (lint compiled sources) |
| Annotation processors | `jot-processors` | `beforeCompile` (configure processor path) |
| Spring Boot repackage | `jot-spring-boot` (future) | `afterPackage` (repackage JAR) |

Internal plugins use the exact same `JotPlugin` interface. External plugin authors have identical capabilities.

### 11.7 Design Principles

**Plugins extend the build lifecycle, not the CLI.** No custom commands. The `jot` namespace is curated and consistent. Plugins that need a CLI ship their own tool or use `jot tool run`.

**Hooks for reactions, tasks for productions.** If your plugin *reacts* to something (add a JVM flag, postprocess a JAR, attach an agent), use a hook. If your plugin *produces* files from inputs (generate code from schemas), use a task.

**Convention over configuration.** Task outputs are auto-wired based on file type. No manual lifecycle phase binding.

**Isolation by default.** Plugins can't share mutable state with each other. Each has its own classloader. Communication is through jot's event system, not direct references.

---

## 12. Workspace / Multi-Module

```
shopflow/
├── jot.toml                    # workspace root + [toolchains] + group
├── jot.lock                    # single lockfile
├── libs.versions.toml          # shared catalog
├── domain/
│   ├── jot.toml
│   └── src/main/java/
├── order-service/
│   ├── jot.toml
│   └── src/main/kotlin/
├── storefront-bff/
│   ├── jot.toml
│   └── src/main/{java,kotlin}/
└── legacy-client/
    ├── jot.toml
    └── src/main/java/
```

**Workspace root:**
```toml
[workspace]
members = ["domain", "order-service", "storefront-bff", "legacy-client"]
group = "com.shopflow"

[toolchains]
java = "21"
kotlin = "2.1.0"
```

**domain/jot.toml** — pure Java library:
```toml
[project]
name = "shopflow-domain"
version = "1.0.0"

[dependencies]
jackson-core = { catalog = "jackson-core", exported = true }

[test-dependencies]
junit = { catalog = "junit" }
```

**order-service/jot.toml** — Kotlin with multiple binaries:
```toml
[project]
name = "shopflow-order-service"
version = "1.0.0"

[toolchains]
graalvm = { version = "21", variant = "ce" }

[[bin]]
name = "server"
main-class = "com.shopflow.orders.OrderServiceApplication"

[[bin]]
name = "migrate"
main-class = "com.shopflow.orders.cli.MigrateCommand"
dist = "native-image"

[[bin]]
name = "seed"
main-class = "com.shopflow.orders.cli.SeedData"

[dependencies]
domain = { path = "../domain" }
spring-web = { catalog = "spring-web" }
picocli = "info.picocli:picocli:4.7.6"
postgres = { coords = "org.postgresql:postgresql:42.7.0", scope = "runtime" }

[test-dependencies]
spring-test = { catalog = "spring-test" }
domain-test = { path = "../domain", include = "test-support" }
```

**legacy-client/jot.toml** — Java 8:
```toml
[project]
name = "legacy-client"
version = "1.0.0"

[toolchains]
java = "8"
```

```bash
$ jot build
  → domain (java 21, javac)... target/shopflow-domain-1.0.0.jar
  → order-service (java 21 + kotlin 2.1.0)
    → target/shopflow-order-service-1.0.0.jar (library)
    → target/bin/server (fat-jar)
    → target/bin/migrate (native binary)
    → target/bin/seed (fat-jar)
  → storefront-bff (java 21 + kotlin 2.1.0, mixed)... done
  → legacy-client (java 8, javac)... target/legacy-client-1.0.0.jar

$ jot fmt
  → domain: 15 java files
  → order-service: 22 kotlin files
  → storefront-bff: 8 java + 12 kotlin files
  → legacy-client: 6 java files

$ jot lint
  → domain: 0 issues (PMD)
  → order-service: 1 warning (detekt)
  → storefront-bff: 0 issues (PMD + detekt)
  → legacy-client: 0 issues (PMD)
```

---

## 13. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        jot CLI (Rust)                        │
│                     Single native binary                     │
├──────────┬──────────┬───────────┬───────────┬───────────────┤
│Toolchain │ Resolver │ Builder   │ Runner    │ Observability │
│  Manager │          │           │           │               │
├──────────┼──────────┼───────────┼───────────┼───────────────┤
│ JDKs     │ PubGrub  │ javac/    │ Project   │ jot inspect   │
│ kotlinc  │ Parallel │ kotlinc   │ --bin sel.│ jot doctor    │
│ GraalVM  │ HTTP/2   │ orchestr. │ Tool exec.│ jot profile   │
│          │ Catalogs │ Increment.│           │  cpu/heap/    │
│          │ + cache  │           │           │  gc/threads   │
├──────────┴──────────┴───────────┴───────────┴───────────────┤
│                   Plugin Host IPC Bridge                     │
├─────────────────────────────────────────────────────────────┤
│                   Plugin Host JVM (managed)                  │
│  ┌─────────────────────┐  ┌───────────────────────────────┐ │
│  │  Internal plugins:  │  │  External plugins (from Maven │ │
│  │  • jot-junit        │  │  Central or local path):      │ │
│  │  • jot-fmt          │  │  • protobuf, openapi          │ │
│  │  • jot-lint         │  │  • spring-devtools            │ │
│  │  • jot-processors   │  │  • fabric-loom (future)       │ │
│  └─────────────────────┘  └───────────────────────────────┘ │
│              Hook Registry + Task DAG Scheduler              │
├─────────────────────────────────────────────────────────────┤
│                    Global Cache (~/.jot/)                     │
│     Content-addressable: JDKs │ JARs │ .class │ resolve      │
└─────────────────────────────────────────────────────────────┘
```

---

## 14. Command Surface

```
jot init [name]              Create new project (interactive)
jot run [--bin name]         Run binary (default: first [[bin]])
jot build                    Compile + package library + binaries
jot test [--filter pattern]  Run tests
jot bench [--filter]         Run JMH benchmarks (src/bench/)
jot bench --compare          Diff against previous benchmark run
jot add <dep>                Add dependency
jot remove <dep>             Remove dependency
jot update [key]             Update catalog versions
jot lock                     Regenerate lockfile
jot tree [--workspace]       Dependency tree
jot audit                    Scan deps for known CVEs (OSV database)
jot audit --fix              Auto-bump to patched versions
jot audit --ci               Exit 1 on critical/high vulns (CI gate)
jot doctor                   Static diagnostics (deps, versions, lockfile)
jot doctor --runtime [--bin] Runtime classpath analysis (duplicates, splits, shadows)
jot publish                  Publish to Maven Central (requires group)
jot clean                    Clear build cache
jot fmt [--check]            Format (built-in)
jot lint [--fix]             Lint (built-in)

jot inspect <class>          Bytecode disassembly
jot inspect --decompile      Decompile to source
jot inspect --size           Class file size breakdown
jot inspect --deps           Class-level dependency graph

jot profile cpu [--bin]      CPU sampling via JFR → flame graph
jot profile heap [--bin]     Heap triage → top retained objects
jot profile gc [--bin]       GC log analysis → pause summary
jot profile threads [--bin]  Thread dump → state summary, deadlock detection

jot java list                List installed JDKs
jot java install <ver>       Install JDK
jot java pin <ver>           Pin JDK version (nearest jot.toml; --workspace for root)

jot tool run <pkg> [--bin]   Run tool ephemerally
jot tool install <pkg>       Install tool globally

jot import pom.xml           Migrate from Maven
jot import build.gradle      Migrate from Gradle

jot self update              Update jot
```

---

## 15. Case Studies

### Case Study 1: Spring Boot Web Service

```toml
[project]
name = "bookshelf-api"
group = "com.bookshelf"
version = "1.0.0"
main-class = "com.bookshelf.BookshelfApplication"

[toolchains]
java = "21"

[dependencies]
spring-bom = { catalog = "spring-bom", bom = true }
spring-web = { catalog = "spring-web" }
spring-jpa = { catalog = "spring-jpa" }
postgres = { coords = "org.postgresql:postgresql:42.7.0", scope = "runtime" }

[test-dependencies]
spring-test = { catalog = "spring-test" }

[processors]
lombok = "org.projectlombok:lombok:1.18.34"
```

```bash
$ jot run
  → Installing JDK 21... done (2.3s)
  → Resolving 47 deps... done (1.1s)
  → Compiling 23 java files... done (3.4s)
  → Started BookshelfApplication in 2.1 seconds
```

### Case Study 2: CLI Tool with Native Binary

```toml
[project]
name = "csvq"
group = "dev.csvq"
version = "0.5.0"
main-class = "dev.csvq.Main"
dist = "native-image"

[toolchains]
java = "21"
graalvm = { version = "21", variant = "ce" }

[dependencies]
picocli = "info.picocli:picocli:4.7.6"
duckdb = "org.duckdb:duckdb_jdbc:1.1.3"
```

```bash
$ jot build
  → target/csvq-0.5.0.jar (library)
  → target/bin/csvq (native binary, 28 MB)

$ jot tool run dev.csvq:csvq -- query "SELECT * FROM './data.csv'"
```

### Case Study 3: Multi-Module Monorepo

See Section 11 — full workspace with pure Java library, Kotlin service (3 binaries), mixed module, and Java 8 legacy module.

### Case Study 4: Desktop Application

```toml
[project]
name = "markdown-studio"
group = "dev.mdstudio"
version = "2.0.0"
main-class = "dev.mdstudio.Main"
dist = "jlink"

[toolchains]
java = "21"
kotlin = "2.1.0"

[dependencies]
javafx-controls = { coords = "org.openjfx:javafx-controls:21.0.5", classifier = "${os.name}" }
flexmark = "com.vladsch.flexmark:flexmark-all:0.64.8"
coroutines = { catalog = "$kotlin.coroutines-core" }
```

### Case Study 5: Open Source Library (Java 8)

```toml
[project]
name = "retry4j"
group = "io.github.pavitra"
version = "1.3.0"

[toolchains]
java = "8"

[dependencies]
slf4j = "org.slf4j:slf4j-api:2.0.16"

[test-dependencies]
junit = "org.junit.jupiter:junit-jupiter:5.11.0"

[publish]
license = "Apache-2.0"
description = "Simple, composable retry logic for Java 8+"
url = "https://github.com/pavitra/retry4j"
scm = "https://github.com/pavitra/retry4j.git"
developer = { name = "Pavitra", email = "pavitra@example.com" }
```

---

## 16. Design Decisions Log

| # | Decision | Resolution | Rationale |
|---|---|---|---|
| 1 | Implementation language | Rust | <5ms startup, single binary, proven by uv/bun/Ruff |
| 2 | Project model | Library + `[[bin]]` (Cargo-style) | Multiple executables from same codebase |
| 3 | Simple projects | `main-class` + `dist` sugar at project level | Desugars to single `[[bin]]` |
| 4 | Language declaration | Inferred from `[toolchains]` + source files | No `lang` field. No redundancy |
| 5 | Kotlin version | Must be explicit in `[toolchains]` | Breaking changes between releases |
| 6 | Formatting | Built-in: google-java-format (Java), ktfmt (Kotlin) | Opinionated. Pinned to jot release |
| 7 | Linting | Built-in: PMD (Java), detekt (Kotlin) | PMD is standalone CLI; error-prone requires recompilation |
| 8 | Format/lint config | Minimal overrides: `[format]` java-style, `[lint]` config paths | Optional. Defaults are sensible |
| 9 | Toolchains structure | Flat top-level for compilers/runtimes only | Formatters/linters built-in |
| 10 | JDK vendor | Version default, optional `vendor` field | Progressive disclosure |
| 11 | Toolchain catalogs | Declaring toolchain activates `$name` catalog | `kotlin = "2.1.0"` → `$kotlin` |
| 12 | Dist formats | `fat-jar`, `native-image`, `jlink` | Build artifacts only. Docker out of scope |
| 13 | Dist placement | Per-`[[bin]]` field; or project-level for `main-class` sugar | Consistent: dist always applies to a binary |
| 14 | `group` field | Optional. Required only for `jot publish` | No ceremony for non-published projects |
| 15 | `group` inheritance | Workspace root can declare `group`, members inherit | Reduces repetition |
| 16 | Catalog format | Gradle `libs.versions.toml` | Dependabot/Renovate/IDE support |
| 17 | BOMs in catalog | Regular `[libraries]` entry + `bom = true` in jot.toml | Keeps catalog strictly Gradle-compatible |
| 18 | Standalone catalogs | `libs.versions.toml` works for non-workspace projects too | Same mechanism everywhere |
| 19 | Dependency scopes | Default + `exported` + scope + `[test-dependencies]` + `[processors]` | Amper-inspired. Default right 90% |
| 20 | Multi-module | Cargo-style workspaces | Shared catalog, single lockfile |
| 21 | Path deps | Explicit paths | Known breakage > silent breakage |
| 22 | Test sharing | `include = "test-support"` convention | Replaces Gradle's `testFixtures` |
| 23 | Toolchain override | Workspace default + per-module key override | Inherited unless overridden |
| 24 | Source dirs | Maven convention by default, `source-dirs`/`test-dirs` override | Convention over configuration |
| 25 | Multi-binary publish | `META-INF/jot-binaries.json` in JAR | Enables `jot tool run <pkg> --bin <n>` |
| 26 | `jot java pin` | Writes to nearest `jot.toml`; `--workspace` flag for root | Least-surprise default |
| 27 | `jot inspect` | Wraps javap + embedded decompiler | Bytecode/class analysis without leaving terminal |
| 28 | `jot profile` | Wraps JFR, jcmd, jmap, jstack, GC logs | Terminal-first UX around existing JDK tech |
| 29 | `jot doctor --runtime` | Static classpath analysis for duplicates/splits/shadows | Catches ClassNotFoundException-class bugs before runtime |
| 30 | Observability philosophy | Wrap, don't reinvent. Terminal-first. Production-safe | No custom agents. Actionable suggestions |
| 31 | `jot bench` | JMH wrapper, src/bench/ convention, opinionated defaults | Zero-setup benchmarking. Results persisted, --compare opt-in |
| 32 | `jot audit` | OSV database, zero-config, --fix auto-patches | Same model as cargo audit / npm audit |
| 33 | REPL | Dropped | Rarely used in practice. People write a main method instead |
| 34 | Single-file scripting | Dropped | JBang exists. jot is a project tool |
| 35 | JPMS | Auto-detected from `module-info.java` | Abstracted away |
| 36 | Warm compilation daemon | Deferred | No benchmark data yet |
| 37 | Gradle Module Metadata | Deferred | Revisit if widely needed |
| 38 | Plugin execution model | Hybrid: hooks (lifecycle events) + tasks (code generation) | Hooks for 90% (reactions), tasks for 10% (productions). Best of Vite + Gradle |
| 39 | Plugin hosting | JVM via IPC (JSON over pipe) with warm daemon | JVM can't share memory with Rust. IPC overhead negligible for build events. Isolation |
| 40 | Plugin custom commands | **Not allowed** | CLI namespace is jot's. Plugins extend build lifecycle, not CLI. No namespace pollution |
| 41 | Internal dogfooding | junit, fmt, lint, processors are internal plugins | Same API as external. Ensures plugin system is powerful enough |
| 42 | Task auto-wiring | Tasks wired into lifecycle by output file type | .java/.kt outputs → before compilation. Resources → before packaging. No manual binding |
| 43 | Plugin isolation | Per-plugin classloader in JVM host | Prevents dependency conflicts between plugins |
| 44 | Spring Boot | First-class from day one | Most common Java project type |
| 45 | Minecraft modding | Future community target | Largest Java on-ramp |
| 46 | Android | Future target, phased approach | Deeply complex (AGP) |
| 47 | Funding | Open source | Community-driven |

---

## 17. Community Targets

### Day-One: Spring Boot

Spring Boot is the most common Java project type in the world. jot treats it as a first-class citizen from day one, not a framework that happens to work. This means:

- **`jot init --template spring-rest`** scaffolds a production-ready Spring Boot REST API with sensible defaults (web starter, test starter, actuator, Lombok processor)
- **Spring BOM integration** works natively via `{ catalog = "spring-bom", bom = true }` — version alignment across 200+ Spring modules is automatic
- **`$spring` toolchain catalog** (future) provides auto-versioned Spring ecosystem libraries, eliminating the "which version of Spring Security is compatible with Spring Boot 3.4?" problem
- **Spring-specific `jot doctor` checks** — detect common misconfigurations like missing `@SpringBootApplication`, conflicting auto-configuration, or incompatible starter combinations
- **Case Study 1** in this proposal is deliberately a Spring Boot project because it's the benchmark experience

The Spring Boot experience should be the *demo* that sells jot to enterprise teams. If `curl | sh && jot init --template spring-rest && jot run` produces a working REST API in under 15 seconds with zero prerequisites, that's the conversion moment.

### Future: Minecraft Modding

Minecraft's modding community is the single largest pipeline of new Java developers in the world. Today, a first-time modder must install a JDK, understand Gradle, debug ForgeGradle/Fabric Loom plugin errors, configure IDE run configurations, and manage obfuscation mappings — all before writing a single line of mod code.

jot can dramatically improve this on-ramp. The exact form (built-in `jot mc` subcommands, templates, or plugin/extension architecture) is deliberately left undesigned — the Minecraft modding community should signal what they need once jot's core exists. What we know:

- **`jot init --template minecraft-fabric`** (or `minecraft-forge`) would eliminate the setup ceremony
- **`jot run --client`** launching Minecraft with a mod loaded would bypass IDE run configuration complexity
- **Zero-prerequisite onboarding** is the killer UX for a 14-year-old's first mod
- **Long-term:** jot could become the foundation that next-gen mod tooling builds on

The strategic value: today's teenage modder is tomorrow's enterprise Java developer.

### Future: Android

Android is Gradle's biggest captive audience and the most complex JVM build system in existence. The Android Gradle Plugin (AGP) handles resource merging, AAPT2 compilation, DEX bytecode generation, ProGuard/R8 shrinking, APK/AAB packaging, build variants, product flavors, and signing — a scope that dwarfs standard JVM builds.

jot's path into Android is long but plausible:
- **Phase 1:** Android developers use jot for their non-Android JVM modules (shared libraries, backend services) in a mixed workspace
- **Phase 2:** `jot init --template android-app` scaffolds a basic Android project, delegating to Android SDK tools for the Android-specific parts
- **Phase 3:** Native Android build support, potentially replacing AGP for common cases

This is aspirational and depends on jot proving itself in the broader JVM ecosystem first. Mill has demonstrated early Android support, proving it's technically feasible.

### Other Potential Communities

- **Academic/research** — Java is widely taught in universities. jot's zero-setup story could replace painful "install Maven, set JAVA_HOME" onboarding in CS courses.
- **Data engineering** — Apache Spark, Flink, Kafka Streams are JVM-based. Teams with Python-first tooling expectations find Maven/Gradle alienating.

---

## 18. Implementation Plan

### Phase 1: Foundation (Months 1-3)
`[toolchains]` parsing, JDK manager, `jot init` (with templates, including `spring-rest`), project compilation, `jot run`, global cache, source directory convention, **plugin host JVM + hook API + internal junit/fmt/lint plugins**.

### Phase 2: Dependency Resolution (Months 3-6)
PubGrub resolver, Maven Central client, `libs.versions.toml` parsing, BOM resolution, `jot.lock`, `jot add/remove/tree`, **`[plugins]` section resolution from Maven Central**.

### Phase 3: Build & Test (Months 6-9)
`[[bin]]` + `dist`, Kotlin two-pass compilation, workspaces, `jot import`, incremental compilation, `[processors]` (as internal plugin), **task API for code generation plugins (protobuf, openapi)**.

### Phase 4: Ecosystem (Months 9-12)
Publishing (multi-binary metadata), `jot tool run/install`, `jot doctor` (static), `jot audit` (OSV scanning), Windows, CI templates, **community plugin ecosystem seeding (protobuf, spring-boot-repackage, openapi)**.

### Phase 5: Observability & Performance (Months 12-18)
`jot inspect` (bytecode/decompile), `jot doctor --runtime` (classpath analysis), `jot profile cpu|heap|gc|threads`, `jot bench` (JMH integration), `jot audit --fix`.

### Phase 6: Scale & Community (Months 18+)
Scala/Groovy toolchains, IDE plugins, remote build cache, expanded toolchain catalogs, native Rust formatter/linter, community templates (Minecraft, Spring, Quarkus), extension/plugin architecture.

---

## 19. Risk Analysis

| Risk | Severity | Mitigation |
|---|---|---|
| Maven POM compatibility | High | Test suite against top 1000 packages |
| Adoption barrier | High | Greenfield focus. Gradle catalogs. `jot import` |
| Plugin API stability | High | Dogfood with internal plugins before opening to community. Semver the API |
| Compilation orchestration | Medium | Source-file detection. Two-pass mixed Java/Kotlin |
| Plugin host JVM overhead | Medium | Warm daemon reuse. Lazy startup (no JVM if no plugins declared) |
| Scope creep (Minecraft, Android) | Medium | Community signals first. Extension via plugins, not core bloat |
| Performance | Low | Rust floor. Minimize everything around JVM compilation |

---

## 20. Why Rust, Why "jot"

**Rust:** <5ms startup. Single binary. ~10MB memory. Proven: uv, bun, Ruff.

**"jot":** Short, lowercase, fast to type. `jot run` = 7 keystrokes.
