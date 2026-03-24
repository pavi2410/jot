# ADR 001: Do Not Add `jot check` Command

**Date:** 2026-03-25
**Status:** Accepted

## Context

`cargo check` in Rust compiles code without producing output artifacts. It is significantly faster than `cargo build` because Rust's compilation pipeline includes expensive code generation and linking phases — `cargo check` stops before those.

The question arose whether `jot` should offer an equivalent `jot check` command.

## Decision

Do not add a `jot check` command.

## Reasoning

In Java, the compilation pipeline does not have a separate code generation or linking phase analogous to Rust. `javac` produces `.class` bytecode directly, and JAR packaging is a cheap zip operation over those class files. As a result:

- `jot check` would save negligible time compared to `jot build`
- Compilation errors already surface immediately via `jot build`
- The semantic distinction ("validate without producing artifacts") does not justify a dedicated command when the cost difference is minimal

## Consequences

Users who want to validate their code compiles should use `jot build`. The output artifacts (JAR files) are a low-cost side effect.

If future profiling reveals that fat JAR creation becomes a meaningful bottleneck, this decision can be revisited.
