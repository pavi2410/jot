# proto-Jot

This small Java prototype checks whether Jot can build a Java implementation
of itself. It reads a narrow subset of `jot.toml`, compiles Java sources with
`javac`, and packages an executable JAR with `jar`. It has no external runtime
dependencies.

## Bootstrap and self-build

From the repository root, install the recent Java toolchain with the current
Rust Jot implementation, then use it to build the prototype:

```sh
cargo run -p jot --manifest-path Cargo.toml -- toolchain install java@27
cd proto-jot
cargo run --manifest-path ../Cargo.toml -p jot -- build
```

Then use the JAR produced by that build to rebuild the same project. Set
`JAVA_HOME` to the Java 27 installation shown by `jot toolchain list`:

```sh
export PATH="$JAVA_HOME/bin:$PATH"
java -jar target/proto-jot-0.1.0.jar build
```

The prototype supports `build`, `run`, and `clean`, and accepts `--project` to
select a project directory. Build source directories and the Java release are
read from `jot.toml`. It is still limited to one module, Java sources, and no
external dependencies. Its TOML reader intentionally supports only the string
values and string arrays this project needs today.
