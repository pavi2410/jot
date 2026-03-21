# multi-module-workspace

Multi-module workspace example for jot.

Modules:
- domain: shared library
- api: simple web server
- cli: command-line entrypoint

## Commands

```bash
jot build
jot test
jot run --module api
jot run --module cli -- --help
```
