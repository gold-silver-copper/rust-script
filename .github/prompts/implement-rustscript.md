# Implement `rustscript`: an embeddable Rust-subset interpreter

You are a senior Rust language-tooling engineer. Implement `rustscript`, a
production-quality, resource-bounded scripting engine for a strict subset of
Rust 2024.

Deliver complete compiling code, tests, examples, fuzz targets, documentation,
a browser-compatible WASM package, and a native differential-testing harness.
Do not stop at architecture notes, scaffolding, pseudocode, placeholder modules,
`todo!()`, or `unimplemented!()`.

Work in the current repository. Do not create a nested repository or project
directory.

This prompt refines the complete language and testing specification in
[GitHub Issue #2](https://github.com/gold-silver-copper/rust-script/issues/2).
The issue's exact language grammar, semantics, examples, invalid corpus, test
matrix, typed generator, differential oracle, artifact requirements, shrinker,
and acceptance criteria remain normative unless this document explicitly
overrides an implementation choice.

## Explicit overrides of Issue #2

This document supersedes the following Issue #2 requirements:

- Replace the handwritten lexer and Pratt/precedence-climbing parser with a
  bounded project-owned ASCII preflight scanner followed by `ra_ap_syntax`.
- Replace the ban on full Rust parsers with the single-parser policy below.
- Replace the `rustscript-core` std-only rule with the dependency allowlist in
  this document.
- Replace the three-crate workspace layout with the four-crate layout below,
  including `rustscript-wasm`.
- A public standalone `lex` function and project-owned token enum are no longer
  required. The required frontend APIs are byte-oriented parsing, string
  parsing, checking, running, and canonical formatting. Internal normalized
  tokens may be used where helpful for validation and tests.
- Lexer/parser tests and completion checkboxes in Issue #2 apply to the combined
  preflight, `ra_ap_syntax`, whitelist-lowering frontend rather than to a
  handwritten parser implementation.
- The no-unsafe rule applies to project-owned workspace source, as clarified
  below, rather than claiming that every transitive dependency is unsafe-free.

All semantic behavior, language restrictions, resource limits, diagnostics,
tests, differential properties, examples, and artifact requirements from Issue
#2 remain in force.

## Priorities

In descending order:

1. Every accepted program is valid Rust 2024.
2. Interpreter behavior matches native Rust for the supported subset.
3. Untrusted input cannot bypass configured resource limits.
4. The semantic engine runs under `wasm32-unknown-unknown`.
5. Diagnostics and public APIs are stable and structured.
6. The native rustc oracle provides reproducible differential testing.
7. Implementation details remain small and maintainable.

When requirements conflict, this priority order controls.

## Compatibility contract

A program is accepted only after it:

1. Passes byte, encoding, size, and nesting preflight checks.
2. Parses as Rust 2024 without syntax errors.
3. Contains only whitelisted subset constructs.
4. Lowers successfully into the project-owned semantic AST.
5. Passes the project-owned type checker.

The defining invariant is:

> Every program accepted by `rustscript` must compile with
> `rustc --edition=2024`. Every accepted program that terminates successfully
> without hitting a configured safeguard must produce exactly the same stdout
> bytes as native execution compiled with overflow checks enabled.

The reverse is intentionally false: `rustc` accepts far more programs than
`rustscript`.

Fuel, call-depth, output, source-size, token-count, AST-depth,
compiler-timeout, and execution-timeout failures are implementation safeguards,
not statements about Rust semantics.

## Parser strategy

Use `ra_ap_syntax` as the only Rust parser.

Do not expose `ra_ap_syntax` types through the public API. Its syntax tree is an
untrusted frontend representation that must be validated and lowered into the
project-owned AST.

The pipeline is:

```text
source bytes
    -> UTF-8 and ASCII validation
    -> source-size and delimiter-depth preflight
    -> ra_ap_syntax Rust 2024 parse
    -> reject all parser errors and ERROR nodes
    -> enforce lexical and syntactic whitelist
    -> enforce token/function/parameter/AST-depth limits
    -> lower into project-owned AST
    -> project-owned type checker
    -> project-owned interpreter or canonical formatter
```

Parsing requirements:

- Parse using Rust 2024 edition mode.
- Reject the source if `ra_ap_syntax` reports any error.
- Never interpret a recovered or partially valid syntax tree.
- Reject every node and token not explicitly allowed by the subset.
- Reject attributes, documentation comments, block comments, unsupported
  literals, raw identifiers, Unicode, unsupported paths, macros other than the
  fixed `println!` form, and every other forbidden construct.
- Preserve inclusive-exclusive byte spans when lowering.
- Do not use rust-analyzer semantic analysis, name resolution, type inference,
  macro expansion, or HIR.
- Do not use parser-specific AST nodes after lowering.
- Never use `unwrap` or `expect` on user-controlled parsing paths.
- Invoke the third-party parser behind `std::panic::catch_unwind` and convert a
  parser unwind into a structured parse diagnostic. No dependency panic may
  cross the public native or WASM API boundary.
- Keep the core and WASM build profiles unwind-capable; do not set their panic
  strategy to `abort`. The oracle's `-C panic=abort` flag applies only to
  generated native comparison programs.

Because `ra_ap_syntax` is an error-recovering parser, merely obtaining a syntax
tree does not mean the source is accepted.

## Preflight safeguards

Before invoking the parser:

- Reject inputs larger than 1 MiB.
- Reject invalid UTF-8.
- Reject non-ASCII bytes.
- Enforce maximum delimiter nesting of 256.
- Enforce a maximum of 100,000 token-like units before invoking
  `ra_ap_syntax`; stop scanning immediately when the limit is exceeded.
- Recognize enough comment and string context that delimiters inside supported
  line comments or the exact `"{}"` token do not affect nesting.
- Reject clearly forbidden block comments and documentation comments at their
  first useful span.
- Implement preflight as a bounded, iterative scanner. It must not recurse,
  allocate proportionally beyond the source limit, or continue after any limit
  is exceeded.

After parsing and before lowering:

- Maximum syntax tokens: 100,000.
- Maximum functions: 1,024.
- Maximum parameters per function: 256.
- Maximum lowered AST nesting: 256.

The source-size, token-work, delimiter-depth, and panic-containment safeguards
protect the third-party parser boundary; post-parse limits protect later phases.

## Project-owned semantic implementation

The following remain fully project-owned:

- Subset whitelist and lowering.
- Semantic AST.
- Names and scopes.
- Static type checker.
- Control-flow analysis.
- Runtime values and environments.
- Tree-walking interpreter.
- Checked arithmetic.
- Runtime safeguards.
- Canonical pretty-printer.
- Structured diagnostics.
- Differential comparison logic.
- Typed program generator.
- Artifact writer and shrinker.

The canonical formatter must operate on the project AST. Do not print the
rust-analyzer tree and do not use a general Rust formatter. This ensures it can
emit only supported syntax and preserves the round-trip invariant.

## Workspace

```text
.
├── Cargo.toml
├── Cargo.lock
├── README.md
├── crates/
│   ├── rustscript-core/
│   ├── rustscript-cli/
│   ├── rustscript-wasm/
│   └── rustscript-difftest/
├── examples/
└── fuzz/
```

### `rustscript-core`

The WASM-safe semantic engine contains preflight validation, parsing and
whitelist lowering, the semantic AST, type checking, interpretation, canonical
formatting, and structured diagnostics.

It must not use filesystem, process, environment, terminal, networking, clock,
thread, or randomness APIs.

### `rustscript-cli`

This native package produces a binary named `rustscript`. It owns file access,
CLI argument handling, human-readable diagnostic rendering, and writing
interpreter output.

### `rustscript-wasm`

This is a thin `cdylib` and `rlib` adapter exposing the core to JavaScript.

Expose operations equivalent to:

```text
check(source, limits)
run(source, limits)
ast(source, limits)
format(source, limits)
```

Return structured JavaScript values through `serde-wasm-bindgen`, including the
phase, message, byte span, line and column, output bytes, and step count. Do not
duplicate parsing or execution logic in this crate.

### `rustscript-difftest`

This native library and binary contains the rustc subprocess oracle, compiler
and execution timeouts, typed deterministic generator, differential comparison,
failure artifacts, reproduction, and AST-aware shrinking.

This crate is intentionally not WASM-compatible because it spawns `rustc` and
native executables.

## Dependency policy

Centralize dependency versions under `[workspace.dependencies]` and commit
`Cargo.lock`.

Use this dependency set unless a documented implementation blocker requires a
change:

```toml
[workspace.package]
edition = "2024"
rust-version = "1.95"

[workspace.dependencies]
rustscript-core = { path = "crates/rustscript-core" }

ra_ap_syntax = "0.0.342"

annotate-snippets = {
    version = "0.12.16",
    default-features = false,
    features = ["std"],
}
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.150"

wasm-bindgen = "0.2.126"
serde-wasm-bindgen = "0.6.5"
wasm-bindgen-test = "0.3.76"

tempfile = "3.27.0"
wait-timeout = "0.2.1"
rand_chacha = "0.10.0"
rand_core = "0.10.1"

proptest = {
    version = "1.11.0",
    default-features = false,
    features = ["std", "bit-set"],
}
```

`ra_ap_syntax 0.0.342` declares Rust 1.95 as its minimum supported Rust
version. Update the dependency and workspace MSRV together.

Package-level dependency boundaries:

```text
rustscript-core:
    ra_ap_syntax
    serde                 optional

rustscript-cli:
    rustscript-core
    annotate-snippets

rustscript-wasm:
    rustscript-core        with serde feature
    serde
    serde-wasm-bindgen
    wasm-bindgen

rustscript-difftest:
    rustscript-core
    rand_chacha
    rand_core
    serde
    serde_json
    tempfile
    wait-timeout

tests and fuzzing only:
    proptest
    wasm-bindgen-test
    arbitrary = 1.4.2
    libfuzzer-sys = 0.4.13
```

Do not add:

- `syn`, `prettyplease`, `proc_macro2`, or `quote`.
- `logos`, `chumsky`, `nom`, `winnow`, `pest`, or `lalrpop`.
- `tree-sitter` or `tree-sitter-rust`.
- Direct `rowan` or rustc-lexer dependencies.
- `rustc_private`, `rustc_driver`, or other rustc internals.
- `clap`, `tokio`, `rayon`, or another framework or runtime.
- `anyhow` in library APIs.
- `rhai`, `rune`, `wasmtime`, `wasmer`, or another scripting engine.
- A second parser, formatter, AST framework, or diagnostic framework.
- Direct dependencies in `rustscript-core` or `rustscript-wasm` that require
  unavailable OS services at runtime. A required transitive crate may contain
  native or thread-related code only when the selected feature set compiles for
  `wasm32-unknown-unknown` and the WASM execution path does not invoke it.

All workspace source must use `#![forbid(unsafe_code)]`. This restriction
applies to project-owned crates. Transitive dependencies may contain unsafe
internals and must be reviewed as part of dependency selection; they are not
covered by the workspace-source prohibition.

## WASM requirements

The semantic engine must compile with:

```text
cargo check -p rustscript-wasm --target wasm32-unknown-unknown
```

Add `wasm-bindgen-test` coverage for:

- Valid parsing and checking.
- Interpreter execution.
- Exact output bytes.
- Structured diagnostics.
- Formatting.
- Fuel exhaustion.
- Output exhaustion.
- Deterministic repeated execution.

Native-only dependencies must not appear in the `rustscript-wasm` target graph.

The browser package does not run rustc differential tests. Differential
conformance is established in native CI before publishing the WASM artifact.

## Diagnostics

Core diagnostics are structured data. Terminal rendering belongs only to the
CLI.

Every diagnostic contains:

- Phase.
- Concise message.
- Optional byte span.
- Optional file name.
- Lazily calculated one-based line and column.

The WASM adapter serializes this structure. The CLI renders it with
`annotate-snippets`. Do not compare diagnostic prose with rustc.

## Verification

Completion requires all of:

```text
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cargo check -p rustscript-wasm --target wasm32-unknown-unknown
wasm-pack test --node crates/rustscript-wasm

cargo run -p rustscript-difftest -- --seed 1 --cases 1000

cargo +nightly fuzz build parser_bytes
cargo +nightly fuzz build ast_roundtrip
```

Ordinary tests must include a deterministic 25-to-50-case differential smoke
corpus using the installed rustc.

Do not claim that a fuzz target, WASM test suite, or large differential run was
executed unless it actually completed.

## Completion report

At completion, report:

- Final workspace structure.
- Dependency graph and any deviations from the allowlist.
- Exact commands executed.
- Unit, integration, property, WASM, and differential results.
- Differential seed and case count.
- Fuzz targets built and targets actually run.
- Independent full-diff review findings and resolutions.
- Known limitations or blockers.

The implementation is incomplete while required checks fail, the WASM target
does not build, generated accepted programs are rejected by rustc,
interpreter/native output differs, confirmed high-severity findings remain, or
required work is represented by placeholders.
