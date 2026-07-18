# rustscript

`rustscript` is an embeddable, resource-bounded interpreter for a strict subset
of Rust 2024. Acceptance is intentionally one-way: every accepted program must
compile as Rust 2024, while most valid Rust is outside the profile. A successful
interpreter run must produce exactly the same stdout bytes as native execution
compiled with overflow checks.

The project is suitable for deterministic scripting and differential language
testing; it is not a Rust compiler, macro engine, or sandbox for arbitrary Rust.

## Workspace

- `rustscript-core`: WASM-safe byte validation, rust-analyzer frontend, subset
  checker/checked IR, interpreter, formatter, and diagnostics.
- `rustscript-cli`: file I/O, commands, terminal diagnostics, and stdout.
- `rustscript-wasm`: `wasm-bindgen` adapter plus replaceable Web Worker host.
- `rustscript-difftest`: rustc subprocess oracle, typed generator, comparison,
  replay, artifacts, and category-preserving reduction.
- `examples`: normative programs with exact expected output.
- `fuzz`: independent `cargo-fuzz` package with byte and typed-IR targets.

See [the tooling reuse audit](docs/tooling-reuse.md) for the rust-analyzer API
map and the measured reason the HIR/type-inference stack is not embedded. See
[the regression corpus](docs/regression-corpus.md) for minimized bug-category
coverage and dependency fuzz findings.

## Exact language profile

Source is at most 1 MiB, valid UTF-8, and ASCII-only. Space, tab, CR, LF, and
ordinary `//` comments are accepted. Attributes, doc/block comments, raw
identifiers, Unicode, strings other than the print placeholder, character/byte
literals, numeric bases, floats, and unsupported punctuation are rejected.

The only source types are `i64`, `bool`, and `()`. Integers use decimal
`DIGITS_i64` spelling with no internal separators. `i64::MIN` cannot be written
directly in version 0. Arithmetic uses checked `i64` operations and reports
overflow, negation overflow, and division/remainder traps rather than wrapping.

Programs contain free functions and exactly one `fn main() { ... }`. Helpers
may have immutable typed parameters and `i64`, `bool`, `()`, or implicit-unit
returns. Signatures are visible in any order; recursion is supported; `main`
cannot be called. Locals use initialized `let`/`let mut`, optional exact type
annotations, lexical shadowing, and bare-name assignment to mutable bindings.

Supported control flow is:

```rust
while condition { /* statements */ }
if condition { /* value */ } else { /* value */ }
return;
return expression;
break;
continue;
```

Supported expressions are literals, locals, bare helper calls, parentheses,
blocks, `if` expressions, unary `-`/`!`, arithmetic `* / % + -`, comparisons
`== != < <= > >=`, and short-circuiting `&& ||`. Comparisons do not chain.

Output is intentionally one intrinsic with one auditable shape:

```rust
println!("{}", expression);
```

The expression must be `i64` or `bool`; bytes are decimal/`true`/`false` plus
one `\n`. There is no string type or general formatting/macro expansion.

Unsupported Rust includes modules, structs/enums/traits, generics, methods,
fields, indexes, arrays/non-unit tuples, paths, casts, references, dereference,
ownership/borrowing, closures, `match`, `loop`, `for`, ranges, labels,
`if let`/`while let`, `?`, and macros other than the fixed print intrinsic.

## Architecture and safeguards

The frontend is reuse-first:

```text
bytes -> UTF-8/ASCII/size checks
      -> ra_ap_parser::LexedStr token policy and delimiter limits
      -> ra_ap_syntax::SourceFile::parse (Edition 2024)
      -> reject every parser error and ERROR node/token
      -> exhaustive typed ast::* admission
      -> lexical-scope checking and direct checked-IR lowering
      -> interpreter or canonical parse-tree/checked-IR emitter
```

Rustscript does not contain a lexer, parser cursor, syntax AST, precedence
table, or handwritten line index. `ParsedProgram` keeps the rust-analyzer tree
and `LineIndex` opaque; `CheckedProgram` is opaque resolved executable IR with
span-insensitive structural comparison for testing.

Default frontend limits are 1 MiB source, 100,000 lexer tokens, delimiter depth
256, 200,000 syntax elements, syntax/IR depth 256, 1,024 functions, and 256
parameters per function. Runtime defaults are 1,000,000 fuel steps, 1,024 call
frames, and 1 MiB stdout. Every statement/expression consumes fuel. Safeguard
errors are resource decisions, not Rust semantics.

Native parsing contains rust-analyzer unwinds and converts them to structured
parse diagnostics. `wasm32-unknown-unknown` aborts on panic, so this cannot be
claimed as recovery inside the Rust function. The JS host runs untrusted calls
in a dedicated Worker, returns `frontend-aborted` if it dies, discards the
instance, and creates a clean replacement. The target-specific
`no_salsa_async_drops` cfg selects synchronous rust-analyzer parse dropping and
is pinned/version-audited.

Canonical formatting walks the admitted rust-analyzer tree for parsed programs
and checked IR for generated/reduced programs. It emits only the supported
profile, uses four-space indentation, retains mutability/type and explicit
helper return annotations, discards comments/whitespace, and parenthesizes
composite expressions. It is a small subset emitter because rust-analyzer
exposes no stable embeddable general formatter; it is not another Rust
formatter.

## CLI

```text
cargo run -p rustscript-cli -- check examples/gcd.rs
cargo run -p rustscript-cli -- run examples/gcd.rs
cargo run -p rustscript-cli -- ast examples/gcd.rs
cargo run -p rustscript-cli -- fmt examples/gcd.rs
```

`check` is silent on success; `run` writes only script bytes; `ast` prints the
pinned rust-analyzer syntax debug tree; `fmt` prints canonical source without
modifying the file. Diagnostics go to stderr. Exit codes are 0 success, 1
source/type/runtime/I/O failure, and 2 usage failure.

## WASM

The adapter exports `check`, `run`, `ast`, and `format`. Responses contain
structured phase/message/span/file/location data and, where applicable, output
bytes, text, and step count. `host.js` and `worker.js` provide the untrusted
worker boundary described above.

```text
cargo check -p rustscript-wasm --target wasm32-unknown-unknown
wasm-pack test --node crates/rustscript-wasm
wasm-pack test --headless --firefox crates/rustscript-wasm --test browser
npm --prefix crates/rustscript-wasm run build
npm --prefix crates/rustscript-wasm test
npm --prefix crates/rustscript-wasm run pack:check
```

## Differential testing

The native oracle invokes `rustc` directly—never a shell—with Edition 2024,
overflow checks, `panic=abort`, no debuginfo, warnings allowed, and color off.
Compiler/native stdout and stderr are drained concurrently with caps; compile
and run have independent timeouts, and timed-out children are killed and
reaped. Interpreter/native output is compared as raw bytes; diagnostic prose is
never compared.

```text
cargo run -p rustscript-difftest -- --seed 1 --cases 1000
cargo run -p rustscript-difftest -- --seed 12345 --case 87
cargo run -p rustscript-difftest -- --replay path/to/failure.rs
cargo run -p rustscript-difftest -- --rustc /path/to/rustc
cargo run -p rustscript-difftest -- --seed 1 --cases 10 --keep-all
```

Generated helpers form a DAG, expressions are generated from expected types,
loops have bounded counters and unavoidable increments, and output is bounded.
Each case checks canonical structural round-trip, idempotence, deterministic
steps/output, rustc acceptance, native success, empty native stderr, and exact
stdout equality.

Failures use non-overwriting `artifacts/differential/seed-N-case-M/`
directories containing original/canonical/minimized source, rust-analyzer
syntax, metadata/reproduction commands, and every interpreter/compiler/native
stream. The reducer emits type-preserving checked-IR candidates and keeps a
smaller candidate only when replay preserves the original category.

## Tests and fuzzing

```text
cargo build --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

cargo +nightly fuzz build parser_bytes
cargo +nightly fuzz build ast_roundtrip
cargo +nightly fuzz run parser_bytes
cargo +nightly fuzz run ast_roundtrip
cargo +nightly fuzz tmin parser_bytes PATH_TO_CRASH
```

`parser_bytes` drives arbitrary bytes through the bounded byte API, exercises
rust-analyzer's parser invariant checker for bounded UTF-8, and checks canonical
round-trip/execution after admission. `ast_roundtrip` builds the shared typed IR
from a decision stream and checks emit/parse/check structural equality and
deterministic interpretation. Neither fuzz target invokes rustc.

Ordinary stable tests remain useful without `cargo-fuzz`: bounded proptests
cover arbitrary bytes/ASCII and typed decisions, all normative examples compile
and execute, and a fixed 25-case corpus runs through installed rustc on every
`cargo test --workspace`.
