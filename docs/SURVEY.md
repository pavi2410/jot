# jot User Expectations Survey

What different levels and types of Java developers would expect from jot.

# Part 1: By Experience Level

---

## Seasoned Java Developer

A veteran Java dev has battle scars from Maven/Gradle and knows the ecosystem deeply.

### Must-haves (table stakes)

- **Gradle/Maven migration path** — `jot migrate` that reads a `pom.xml` or `build.gradle` and generates a `jot.toml`. This is the #1 adoption driver; no one rewrites their build from scratch.
- **Custom repository support** — Corporate proxies (Artifactory, Nexus) are non-negotiable. Private registries with auth (tokens, basic auth, mTLS).
- **Build lifecycle hooks / plugins** — Annotation processing (Lombok, MapStruct, Dagger), bytecode manipulation (JaCoCo, AspectJ), resource filtering, code generation (protobuf, OpenAPI). Without extensibility, jot can only handle toy projects.
- **Profile/environment support** — Different deps/settings for dev vs prod vs test. Environment-specific config overlays.
- **CI/CD friendliness** — Reproducible builds, caching-friendly output, non-interactive mode, exit codes, machine-readable output (JSON/XML), integration with GitHub Actions / Jenkins.
- **BOM / dependency management** — Spring Boot BOMs, platform constraints, exclusion rules, forced versions.
- **Multi-module builds that scale** — Incremental compilation, parallel module builds, build avoidance (skip unchanged modules). Workspaces are a start, but the devil is in performance at 50+ modules.
- **Source/target compatibility control** — `--release` flag, cross-compilation to older JDKs, multi-release JARs.
- **Integration with existing tooling** — IDE support (IntelliJ project generation), test coverage reports, static analysis beyond PMD (SpotBugs, Error Prone, Checkstyle).
- **Fat JAR / shading / shadow** — Packaging an executable uber-jar with dependency relocation is extremely common for microservices.

### Nice-to-haves

- `jot doctor` — Diagnose environment issues (JDK mismatch, corrupt cache, stale locks).
- `jot benchmark` / JMH integration.
- Native image support (GraalVM).
- Build scan / build timeline visualization for performance debugging.
- DAP/JDWP debug launch (`jot run --debug`).

### Likely concerns

- "How does this handle annotation processors?" — Make-or-break for real-world adoption.
- "What about Gradle plugins I depend on?" — e.g., Spring Boot plugin, Quarkus plugin, jOOQ codegen.
- "Can I eject?" — If jot doesn't work, can I export back to Maven/Gradle?

---

## Beginner / Novice Java Developer

A beginner just wants to write Java and see it run. They don't know (or care) what Maven Central is.

### Must-haves (first 5 minutes)

- **Zero-config start** — `jot init` then `jot run` should Just Work with no JDK pre-installed.
- **Helpful error messages** — "Cannot find symbol `Scanner`" should suggest `import java.util.Scanner;`. Compiler errors should have human-friendly explanations, not just raw `javac` output.
- **`jot add` with fuzzy search** — Beginners don't know Maven coordinates. `jot add gson` should find `com.google.code.gson:gson` automatically. Interactive search would be ideal.
- **Built-in REPL or scratch mode** — `jot repl` or `jot run MyFile.java` (single-file execution without a full project). JEP 330+ style.
- **Simple, well-documented templates** — `jot init` templates should have comments explaining the project structure. A `--guided` mode that asks questions interactively.
- **Watch mode** — `jot run --watch` to auto-recompile on save. Instant feedback loop is critical for learning.

### Nice-to-haves

- `jot doc <topic>` — Quick reference for common Java concepts, or link to relevant docs.
- **Curated dependency catalog** — `jot add --search web-framework` showing popular options with brief descriptions.
- **Guided error recovery** — When a build fails, suggest specific fixes, not just dump a stack trace.
- **Example projects** — `jot init --template rest-api` with a working example to modify.
- **`jot test` with clear output** — Green/red test results with actual vs expected values prominently displayed.

### What would scare them away

- Having to understand TOML syntax to add a dependency manually.
- Cryptic Maven resolution errors ("Could not find artifact...").
- Needing to know about classpaths, module paths, or JDK versions to get started.
- No LSP / IDE guidance — beginners lean heavily on autocomplete.

---

# Part 2: By Developer Persona

## CLI Tool Developer

Building command-line applications, data pipelines, batch processors, and dev tooling in Java.

**Examples:** picocli apps, Quarkus CLI, data ETL pipelines, internal tooling scripts.

### Must-haves

- **Single executable packaging** — `jot build --fat-jar` or `jot build --uber-jar` to produce a self-contained JAR. Ideally also `jot build --native` via GraalVM native-image for zero-startup-time binaries.
- **Argument parsing library support** — Easy integration with picocli, JCommander, or airline. Annotation processing support is critical here (picocli uses it for GraalVM reflection config).
- **Single-file execution** — `jot run Script.java` without needing a full project. Useful for quick scripts and prototyping, similar to JBang.
- **Shebang support** — `#!/usr/bin/env jot run` at the top of a `.java` file to make it directly executable from the shell.
- **`jot install`** — Install a CLI tool globally from a jot project (symlink the built artifact to `~/.local/bin` or similar).
- **Startup time awareness** — Options to minimize JVM startup overhead: CDS (Class Data Sharing) archive generation, AOT compilation hints.

### Nice-to-haves

- `jot init --template cli` with picocli pre-configured and a sample command.
- `jot run --args "-- --help"` passthrough for CLI args without ambiguity.
- Cross-compilation of native images for different OS/arch targets.
- Man page generation from code annotations.

### Likely concerns

- "Can I distribute this without requiring users to have Java installed?" — Native image or jlink runtime image support.
- "How fast is cold start vs JBang?" — CLI tools live or die by startup latency.

---

## Web / Backend Developer

Building REST APIs, microservices, web applications, and server-side systems.

**Examples:** Spring Boot services, Quarkus microservices, Jakarta EE apps, Micronaut APIs.

### Must-haves

- **Framework plugin/integration support** — Spring Boot, Quarkus, and Micronaut all have deep build tool integration (dev mode, annotation processing, bytecode transformation). jot needs either a plugin API or first-class support for these.
- **Dev server with hot reload** — `jot run --watch` or `jot dev` that detects changes, recompiles, and restarts the server. Spring DevTools and Quarkus dev mode set the bar here.
- **Annotation processor support** — Non-negotiable. Spring uses them (Spring Configuration Processor), Micronaut is built entirely on them, Quarkus uses them for build-time optimization.
- **Resource handling** — `src/main/resources` with proper classpath inclusion, filtering/variable substitution (e.g., `application.properties` with `${project.version}`).
- **WAR packaging** — For teams deploying to traditional app servers (Tomcat, WildFly). Less common now but still needed in enterprise.
- **Docker-friendly builds** — `jot build --docker` or at minimum a Dockerfile-friendly layered JAR output. Multi-stage build support.
- **Environment/profile configs** — `application-dev.properties` vs `application-prod.properties` style config switching.

### Nice-to-haves

- `jot init --template rest-api` with a working Spring Boot or Micronaut starter.
- `jot init --template graphql` for GraphQL server scaffolding.
- Built-in HTTP client testing (`jot test --integration` that starts the server, runs tests, shuts down).
- OpenAPI spec generation from code.
- Database migration tool integration (Flyway, Liquibase).
- `jot deploy` — Push to common PaaS targets (Docker registry, AWS Lambda, Cloud Run).

### Likely concerns

- "Does this work with Spring Boot 3.x?" — Framework compatibility is the first question.
- "Can I use `spring-boot-starter-*` BOMs?" — BOM support is critical.
- "What about Testcontainers?" — Integration test setup with Docker dependencies.

---

## Desktop / GUI Developer

Building desktop applications with graphical interfaces.

**Examples:** JavaFX apps, Swing applications, SWT/Eclipse RCP, TornadoFX (Kotlin).

### Must-haves

- **JavaFX module support** — JavaFX is modular (JPMS) and requires `--module-path` and `--add-modules` flags. jot needs to handle this transparently when JavaFX dependencies are detected.
- **JPMS (Java Module System) support** — Desktop apps are the most common users of the module system. `module-info.java` compilation, module path vs classpath handling.
- **Platform-specific dependencies** — JavaFX ships separate artifacts per OS (`javafx-controls:linux`, `javafx-controls:mac`). jot needs Maven classifier support to resolve the right platform variant.
- **Application bundling** — `jpackage` integration to produce `.dmg`, `.msi`, `.deb` installers. This is the #1 pain point for desktop Java devs.
- **Resource embedding** — Images, FXML files, CSS stylesheets need to be on the classpath and easily referenced at runtime.
- **`jot run` with GUI** — Should not block on headless environments. Detect and warn if running in a headless context.

### Nice-to-haves

- `jot init --template javafx` with a working hello-world window.
- `jot package` — Wraps `jpackage` with sensible defaults (app icon, version, signing).
- FXML hot reload during development.
- Scene Builder integration guidance.
- Cross-platform packaging (build a Windows installer on macOS via CI).
- `jlink` support to produce minimal custom JRE images bundled with the app.

### Likely concerns

- "JavaFX hasn't been in the JDK since Java 11. Does jot handle the separate SDK?" — Must auto-resolve JavaFX modules.
- "Can I produce a `.app` bundle for macOS without manually scripting `jpackage`?"
- "Swing apps don't use modules. Does jot force JPMS on me?" — Should work with or without `module-info.java`.

---

## Library / SDK Developer

Building reusable libraries published to Maven Central or private repositories.

**Examples:** Utility libraries, API client SDKs, frameworks, annotation processors.

### Must-haves

- **Publishing to Maven Central** — Full Sonatype OSSRH workflow: staging, closing, releasing. GPG signing, POM generation with correct metadata. jot already has `jot publish` — this needs to be battle-tested.
- **Sources JAR + Javadoc JAR** — Required by Maven Central and expected by consumers. jot already generates these.
- **POM customization** — License, SCM URL, developer info, description — all required by Maven Central validation.
- **API compatibility checking** — Detect breaking changes between versions. Something like `japicmp` or `revapi` integration.
- **Multi-target compilation** — Build the same library against Java 8, 11, 17, 21 to verify compatibility. Multi-release JAR support.
- **Dependency scope correctness** — `api` vs `implementation` vs `compileOnly` vs `runtimeOnly` — these matter because they propagate to consumers' classpaths.
- **BOM publishing** — For libraries that ship multiple modules, publishing a BOM so consumers can align versions.
- **Reproducible builds** — Byte-for-byte identical JARs for the same source input. Strip timestamps from JAR entries.

### Nice-to-haves

- `jot init --template lib` with pre-configured publishing metadata.
- `jot publish --dry-run` to validate everything before pushing.
- `jot release` — Bump version, tag, publish in one command.
- Changelog generation from conventional commits.
- `jot compat check` — Compare current API surface against the last release.
- Automatic `module-info.java` generation for JPMS compatibility.
- Snapshot publishing for pre-release testing.

### Likely concerns

- "Does the generated POM pass Maven Central validation?" — Strict requirements on required elements.
- "Can consumers use my library from Maven/Gradle projects?" — Interop is everything for a library.
- "How do I manage transitive dependency versions for my consumers?" — Dependency mediation strategy.

---

## Android Developer

Building Android applications (though Android uses its own build system, crossover exists).

**Examples:** Shared Java/Kotlin modules used in both Android and server-side, annotation processors consumed by Android builds.

### Must-haves

- **Shared module compilation** — Build a pure Java/Kotlin module (no Android dependencies) that can be consumed by both an Android project (via Gradle) and a server project (via jot).
- **Java 8/11 bytecode targeting** — Android's desugaring has limits. Shared modules must target compatible bytecode versions.
- **Kotlin support** — Android development is Kotlin-first. jot's Kotlin compilation must produce artifacts compatible with Android's Kotlin version expectations.
- **AAR is out of scope (and that's fine)** — jot should not try to replace AGP (Android Gradle Plugin). But it should clearly document the boundary.

### Nice-to-haves

- `jot build --target 8` to easily target Java 8 bytecode for Android compatibility.
- Documentation on how to consume jot-built JARs from a Gradle Android project.
- KMP (Kotlin Multiplatform) module support for shared business logic.

### Likely concerns

- "Can I use this for my shared `core` module that both my Spring backend and Android app depend on?"
- "Will the JAR actually work on Android, or will it pull in JDK APIs that aren't available?"

---

## Data / ML Engineer

Using Java for data processing, ML pipelines, and scientific computing.

**Examples:** Apache Spark jobs, Flink pipelines, DL4J models, Tablesaw analysis, Jupyter-style notebooks.

### Must-haves

- **Fat JAR for cluster submission** — `spark-submit` and `flink run` require uber-JARs with all dependencies shaded. Dependency relocation (shading) is critical to avoid classpath conflicts with the cluster's own dependencies.
- **Large dependency trees** — Spark/Flink/Hadoop pull in hundreds of transitive dependencies. Resolution must be fast and correct, with robust conflict resolution.
- **`provided` scope** — Spark/Flink are provided by the cluster runtime. Dependencies must be marked as `provided` so they're excluded from the fat JAR.
- **Scala interop** — Many data frameworks are written in Scala. jot needs to handle Scala library versioning (e.g., `spark-core_2.12` vs `spark-core_2.13`).

### Nice-to-haves

- `jot init --template spark-job` or `jot init --template flink-job`.
- `jot run --spark` that wraps `spark-submit` locally.
- Jupyter/JShell notebook-style scratch execution for exploratory data work.
- `jot build --shade` with relocation rules for common conflicts (Guava, Jackson, etc.).

### Likely concerns

- "The shaded JAR is 200MB. Can jot handle that without OOM?" — Build performance at scale.
- "Spark needs Scala 2.12 artifacts. How do I pin the Scala suffix?" — Classifier/variant handling.
- "I need different dependency sets for `compile` vs `runtime` vs `provided`."

---

## Minecraft Modder

Building mods, plugins, and modpacks for Minecraft using Java.

**Examples:** Fabric mods, Forge/NeoForge mods, Paper/Spigot/Bukkit server plugins, Velocity proxy plugins.

This is one of the largest and most active Java communities. Many modders are young developers for whom Minecraft modding is their first real Java project — making this persona a blend of "beginner" and "specialist."

### Must-haves

- **Dependency on custom Maven repositories** — The Minecraft modding ecosystem runs on non-Central repositories (Fabric Maven, NeoForge Maven, PaperMC repo, SpongePowered, JitPack). jot must support multiple custom repository URLs in `jot.toml` with priority ordering.
- **Minecraft-specific dependency resolution** — Mods depend on specific Minecraft versions and loader versions. Artifacts use classifiers and version ranges heavily (e.g., `fabric-api:0.92.0+1.20.4`). The `+` suffix convention for Minecraft version compatibility needs to resolve correctly.
- **Annotation processor support** — Mixin (the bytecode transformation library used by most mods) uses annotation processors. Fabric and NeoForge both rely heavily on Mixin for patching Minecraft classes at runtime.
- **Obfuscation mapping awareness** — Minecraft's code is obfuscated. Mod toolchains (Fabric Loom, ForgeGradle) deobfuscate Minecraft JARs and remap mod code at build time. This is deeply integrated into current Gradle builds. jot would need either a plugin API to support this or built-in mapping support.
- **Access wideners / access transformers** — Fabric uses access wideners and Forge uses access transformers to modify Minecraft class visibility at compile time. These require build-time processing of special config files.
- **JAR-in-JAR / nested dependencies** — Mods bundle their dependencies inside the mod JAR (Fabric's `jar-in-jar` system) rather than using a flat classpath. jot needs to support this packaging format.
- **`resources/` processing** — Mods have metadata files (`fabric.mod.json`, `mods.toml`, `plugin.yml`) that often need variable substitution (injecting version from the build config).

### Nice-to-haves

- `jot init --template fabric-mod` / `jot init --template paper-plugin` with a working hello-world mod/plugin.
- **Mixin config generation** — Auto-generate or validate `mixins.json` from annotated classes.
- **Multi-loader support** — Build the same mod for both Fabric and NeoForge from a shared codebase (like Architectury does). Workspace support with platform-specific source sets.
- **Dev environment setup** — Download and deobfuscate a Minecraft JAR for development (what Fabric Loom's `genSources` does).
- **Run client/server** — `jot run --client` / `jot run --server` to launch Minecraft with the mod loaded for testing.
- `jot publish --modrinth` / `jot publish --curseforge` — Publish to mod distribution platforms.
- Modpack dependency management and version pinning.

### Likely concerns

- "Can this replace Fabric Loom / ForgeGradle?" — These Gradle plugins do an enormous amount of work (deobfuscation, remapping, run configs). This is the hardest ecosystem to support without a plugin API.
- "Will other modders be able to depend on my mod if I build with jot?" — Interop with Gradle-built mods is essential.
- "I'm 15 and this is my first Java project. Is this easier than Gradle?" — If the answer is yes, adoption could be massive. Minecraft modders are constantly frustrated by Gradle's complexity.
- "Does Mixin work?" — If Mixin annotation processing doesn't work, the mod can't function. Full stop.

### Why this persona matters

- Minecraft is often cited as the single biggest driver of new Java developers.
- The modding community is deeply frustrated with Gradle — build scripts are cargo-culted from templates and break constantly across Minecraft version updates.
- Modders are a vocal, community-driven audience. If jot works for them, word-of-mouth adoption would be significant.
- However, the toolchain requirements (deobfuscation, remapping, Mixin) are highly specialized. A plugin API or partnership with the Fabric/NeoForge teams would likely be needed.

---

# Part 3: Gap Analysis

## By Persona

| Persona | Top Blocker | Key Feature Gap | Current jot Status |
|---------|-------------|-----------------|-------------------|
| Seasoned Dev | Annotation processors, plugin API | Maven/Gradle migration | No migration, no plugin API |
| Beginner | Intimidating errors | Fuzzy dep search, watch mode, REPL | Raw compiler output |
| CLI Dev | Native image / fast startup | Single-file execution, `jot install` | Basic JAR only |
| Web / Backend Dev | Framework integration (Spring, Quarkus) | Hot reload dev server, BOM support | No framework support |
| Desktop / GUI Dev | JPMS + JavaFX module handling | `jpackage` bundling | No JPMS, no jpackage |
| Library Dev | Maven Central publishing workflow | API compat checking, BOM publishing | `jot publish` exists (needs hardening) |
| Android Dev | Bytecode target control | Shared module JAR compatible with AGP | Basic build only |
| Data / ML Engineer | Fat JAR with shading/relocation | `provided` scope, Scala variant handling | No shading support |
| Minecraft Modder | Obfuscation remapping, Mixin support | Custom repos, JAR-in-JAR, plugin API | No custom repos, no plugin API |

## Cross-cutting Themes

Features that appear across multiple personas, ranked by breadth of impact:

| Feature | Personas That Need It | Priority |
|---------|----------------------|----------|
| **Fat JAR / uber-JAR** | CLI, Web, Data | Critical |
| **Annotation processor support** | Seasoned, CLI, Web, Library, Minecraft | Critical |
| **Custom Maven repositories** | Minecraft, Seasoned, Library | Critical |
| **GraalVM native-image** | CLI, Web | High |
| **JPMS (module system)** | Desktop, Library | High |
| **`jlink` custom runtimes** | Desktop, CLI | High |
| **BOM support** | Seasoned, Web, Library | High |
| **Watch / hot reload** | Beginner, Web | High |
| **Shading / relocation** | Data, CLI | Medium |
| **Single-file execution** | Beginner, CLI | Medium |
| **Dependency scopes (provided, api, etc.)** | Data, Library, Web | Medium |
| **Cross-compilation** | Library, Desktop | Medium |
| **Plugin / extensibility API** | Seasoned, Web, Minecraft | Medium |
| **Resource variable substitution** | Web, Minecraft | Medium |

## Key Takeaways

- **Biggest unlock for beginners:** Fuzzy dependency search and watch mode.
- **Biggest unlock for veterans:** Annotation processor support and Maven/Gradle migration.
- **Biggest unlock for web devs:** Spring Boot / Quarkus integration — this is the largest Java audience.
- **Biggest unlock for CLI devs:** Native image and `jlink` for distribution without requiring a JDK.
- **Biggest unlock for library authors:** Hardened Maven Central publishing and API compatibility checks.
- **Biggest unlock for Minecraft modders:** Custom repo support + annotation processors. A plugin API would be needed long-term for full Fabric/Forge integration, but even basic support would win hearts in a community deeply frustrated by Gradle.
- **Universal priority:** Annotation processors and custom Maven repositories cut across nearly every persona.
