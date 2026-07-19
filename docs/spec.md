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

## Priorities

In descending order:

1. Every accepted program is valid Rust 2024.
2. Interpreter behavior matches native Rust for the supported subset.
3. Existing Rust tooling is reused instead of reimplemented wherever its public
   API is suitable.
4. Untrusted input cannot bypass configured resource limits.
5. The semantic engine runs under `wasm32-unknown-unknown`.
6. Diagnostics and public APIs are stable and structured.
7. The native rustc oracle provides reproducible differential testing.
8. Implementation details remain small and maintainable.

When requirements conflict, this priority order controls.

## Compatibility contract

A program is accepted only after it:

1. Passes byte, encoding, size, and nesting preflight checks.
2. Parses as Rust 2024 without syntax errors.
3. Contains only whitelisted subset constructs.
4. Is validated through rust-analyzer's typed syntax APIs.
5. Passes the subset type checker and lowers to a checked executable IR.

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

## Rust-tooling reuse mandate

This is a reuse-first project. Use `ra_ap_syntax` as the authoritative Rust
frontend and `ra_ap_parser::LexedStr` as its companion lexer API. Do not write a
lexer, token enum, recursive-descent parser, Pratt parser, precedence table,
syntax-shaped AST, line index, or generic syntax-tree framework.

`ra_ap_syntax` remains an internal implementation detail so the public API is
not tied to rust-analyzer's rapid release cadence. Internally, retain and walk
its lossless `SourceFile`, typed `ast::*` nodes, `SyntaxNode`/`SyntaxToken`,
`SyntaxKind`, `TextRange`/`TextSize`, operator enums, precedence helpers, and AST
traits for as long as syntax information is needed. Do not immediately copy the
tree into a second untyped AST.

Use the matching `ra_ap_parser` package directly only for `LexedStr`, because
`ra_ap_syntax` uses that lexer but does not re-export its public token stream.
Pin both packages to the exact same release. Do not invoke
`ra_ap_parser::TopEntryPoint` or assemble parser input/output manually;
`ra_ap_syntax::SourceFile::parse` is the sole parsing entry point.

The pipeline is:

```text
source bytes
    -> UTF-8 and ASCII validation
    -> source-size preflight
    -> ra_ap_parser::LexedStr in Edition 2024
    -> lexical errors, lexical whitelist, token count, and delimiter-depth checks
    -> ra_ap_syntax::SourceFile::parse in Edition 2024
    -> reject all parser errors and ERROR nodes
    -> typed ast::* subset-admission visitor
    -> syntax node/function/parameter/depth limits
    -> subset type checker and lowering to checked executable IR
    -> project-owned interpreter or minimal canonical subset emitter
```

Required rust-analyzer API usage:

- Parse only with `SourceFile::parse(source, Edition::Edition2024)` (or the
  exact equivalent variant name in the pinned release).
- Use `Parse::errors()` for parser and built-in syntax-validation errors, then
  explicitly reject any remaining `SyntaxKind::ERROR` node or token.
- Traverse items and constructs through typed `ast::*` nodes and traits such as
  `AstNode`, `HasName`, `HasAttrs`, `HasGenericParams`, `HasArgList`, and
  `HasLoopBody`; use raw `SyntaxNode` traversal only for cross-cutting checks or
  where the typed API has no accessor.
- Use `ast::Expr`, `ast::Stmt`, `ast::Item`, `ast::Type`, and `ast::Pat`
  variants as the exhaustive syntax whitelist. Unknown variants are rejected,
  never silently skipped.
- Use `ast::PrefixExpr::op_kind`, `ast::BinExpr::op_kind`, and the
  `ast::{UnaryOp, BinaryOp, ArithOp, CmpOp, LogicOp}` enums. Do not recognize
  operators by hand-written character matching after lexing.
- Use rust-analyzer path, literal, comment, attribute, token-tree, name, and
  pattern nodes to validate those forms. Inspect token text only for
  distinctions intentionally normalized by the lexer or specific to the
  profile: line versus block/doc comment spelling, raw-identifier spelling,
  decimal `DIGITS_i64`, the exact `"{}"` placeholder, reserved subset names,
  and the fixed `println!` shape.
- Use `SyntaxNode::text_range()`/`SyntaxToken::text_range()` as the source of
  byte ranges. Convert to the public `Span` only at the API boundary.
- Use `SyntaxNode::preorder_with_tokens()` and `WalkEvent` for a single
  iterative syntax-element count/depth pass; do not recurse just to measure the
  rust-analyzer tree.
- Use the rust-analyzer `line-index` crate's `LineIndex` for line/column and
  source-line lookup. Do not implement newline indexing or UTF offset mapping.
- Use `ast::prec` and the AST operator APIs anywhere precedence or parentheses
  must be reasoned about. The subset emitter may conservatively parenthesize
  every composite expression, but it must not introduce a second precedence
  implementation.
- Prefer `ast::make`, `ast::syntax_factory::SyntaxFactory`, and
  `syntax_editor::SyntaxEditor` for generated or reduced syntax when their
  public APIs cover the operation. These constructors are for trusted generated
  data only; do not feed user-controlled fragments to helpers that can panic.

Admission requirements:

- Parse using Rust 2024 edition mode.
- Reject the source if `ra_ap_syntax` reports any error.
- Never interpret a recovered or partially valid syntax tree.
- Reject every node and token not explicitly allowed by the subset.
- Reject attributes, documentation comments, block comments, unsupported
  literals, raw identifiers, Unicode, unsupported paths, macros other than the
  fixed `println!` form, and every other forbidden construct.
- Preserve inclusive-exclusive byte spans from rust-analyzer `TextRange`s when
  lowering to checked IR.
- Do not use rust-analyzer semantic analysis, name resolution, type inference,
  macro expansion, or HIR unless a separate, measured integration demonstrates
  that the public API is stable enough for this project, compiles and executes
  under `wasm32-unknown-unknown`, preserves deterministic resource limits, and
  materially replaces project code. Do not recreate any such facility merely
  to imitate the full Rust compiler.
- Do not retain a second syntax-shaped tree. The checked IR may contain only
  information needed for type-safe execution, diagnostics, deterministic
  emission, generation, and reduction; it must omit trivia, parentheses,
  parser recovery structure, and other syntax already owned by
  `ra_ap_syntax`.
- Never use `unwrap` or `expect` on user-controlled parsing paths.
- On native targets that support unwinding, invoke the third-party parser behind
  `std::panic::catch_unwind` and convert a parser unwind into a structured parse
  diagnostic.
- Do not claim that `catch_unwind` contains panics on
  `wasm32-unknown-unknown`. That target aborts on panic; use the Web Worker
  isolation requirement below for untrusted browser input.

Because `ra_ap_syntax` is an error-recovering parser, merely obtaining a syntax
tree does not mean the source is accepted. Rustscript adds a strict admission
visitor; it does not add another Rust grammar.

### No-duplication rule

Before writing syntax-adjacent code, inspect the pinned rust-analyzer public
APIs. If an existing API already performs the operation, use it. If no suitable
API exists, document the gap next to the minimal custom implementation and add
a focused test. In particular, the implementation must not contain:

- A project `Token`/`TokenKind` model or lexer state machine.
- A parser cursor, grammar functions, Pratt loop, or operator precedence table.
- A public or internal untyped `Program`/`Expression` tree that mirrors
  rust-analyzer's typed AST.
- Manual source slicing to rediscover ranges already exposed by syntax nodes.
- A keyword classifier, literal classifier, comment classifier, or path parser
  that duplicates a rust-analyzer API.
- A handwritten line/column index.
- A second general-purpose Rust formatter or syntax editor.

Maintain `docs/tooling-reuse.md` with a short table of each syntax-adjacent
need, the reused API, and any unavoidable rustscript-specific code. Dependency
upgrades must update this audit and re-check WASM behavior.

## Preflight safeguards

Before lexing:

- Reject inputs larger than 1 MiB.
- Reject invalid UTF-8.
- Reject non-ASCII bytes.

Then construct `ra_ap_parser::LexedStr` once and iterate indices only in the
proven-safe range `0..lexed.len()`:

- Convert every `LexedStr::errors()` entry to a structured lexical diagnostic.
- Enforce a maximum of 100,000 tokens before invoking the full parser; stop
  validation immediately when the limit is exceeded.
- Enforce maximum delimiter nesting of 256 from rust-analyzer token kinds.
- Enforce the lexical subset using `SyntaxKind`, rust-analyzer AST-token
  classifiers where applicable, and token text only for subset-specific exact
  spellings.
- Because `LexedStr` intentionally normalizes raw identifiers to `IDENT` and
  comment forms to `COMMENT`, inspect those token texts to reject `r#...`,
  `/*...*/`, `///...`, and `//!...` at their lexer-provided range. Likewise use
  exact token text for the restricted numeric/string spellings. This is policy
  over already-delimited tokens, not lexical analysis.
- Reject unsupported literal/token kinds, single `&`/`|`, and other forbidden
  forms at their lexer-provided range.
- Do not re-scan comment or string interiors. `LexedStr` already establishes
  token boundaries, so delimiter accounting considers delimiter token kinds
  only.

The only custom byte preflight is bounded UTF-8/ASCII/size validation. The only
custom token pass is policy over rust-analyzer tokens; it is not a lexer.

After parsing and before checked-IR lowering:

- Maximum syntax nodes plus tokens: 200,000, counted through
  `preorder_with_tokens()` as a consistency and memory-work bound.
- Maximum functions: 1,024.
- Maximum parameters per function: 256.
- Maximum typed syntax and checked-IR nesting: 256.

The source-size, lexer-token, lexical-policy, and delimiter-depth safeguards
bound input presented to the parser; post-parse limits protect later phases.
They do not prove that a third-party parser can never abort because of an
internal defect. Native builds contain unwind panics, browser builds use worker
isolation, and fuzzing continuously tests this dependency boundary.

## Project-owned semantic boundary

Only rustscript-specific behavior remains project-owned:

- Subset admission policy over rust-analyzer nodes.
- Compact checked executable IR, not a second Rust syntax AST.
- The subset's names, scopes, and three-type static semantics.
- Control-flow analysis.
- Runtime values and environments.
- Tree-walking interpreter.
- Checked arithmetic.
- Runtime safeguards.
- Minimal canonical subset emitter.
- Rustscript diagnostic schema and error taxonomy.
- Differential comparison logic.
- Typed program generator.
- Artifact writer and shrinker.

There is no stable, embeddable, WASM-compatible general Rust formatter exposed
by `ra_ap_syntax`. Therefore a small subset-only canonical emitter is the one
intentional syntax-adjacent exception. It must consume validated typed syntax or
checked IR, use rust-analyzer operator/precedence information where applicable,
emit only whitelisted forms, and remain far smaller than a Rust formatter. Do
not implement generic Rust formatting, comment preservation, token rewriting,
or unsupported constructs. Document this exception in `docs/tooling-reuse.md`.

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

Use Cargo resolver 2 and a virtual workspace. Exclude `fuzz/` from the normal
workspace and configure it as a standard `cargo-fuzz` package. Put integration
tests under actual packages rather than the virtual root. Implement
`rustscript-difftest` as a reusable library plus a thin binary so tests and the
standalone runner share one oracle, generator, comparator, artifact writer, and
shrinker.

### `rustscript-core`

The WASM-safe semantic engine contains byte preflight, rust-analyzer lexing and
parsing, typed-AST subset admission, checked-IR lowering, type checking,
interpretation, canonical subset emission, and structured diagnostics.

Prefer module boundaries like:

```text
src/
├── frontend/
│   ├── bytes.rs          # size/UTF-8/ASCII only
│   ├── lex_policy.rs     # policy over ra_ap_parser::LexedStr
│   ├── admit.rs          # typed ra_ap_syntax AST whitelist
│   └── mod.rs
├── checked_ir.rs
├── typeck.rs
├── eval.rs
├── emit.rs
├── diagnostic.rs
├── limits.rs
└── lib.rs
```

Do not create project `lexer.rs`, `token.rs`, `parser.rs`, `ast.rs`,
`precedence.rs`, or `line_index.rs` modules. A file may wrap a tooling API only
when its name and documentation make that delegation explicit.

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
failure artifacts, reproduction, and syntax/IR-aware shrinking.

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

ra_ap_syntax = "=0.0.342"
ra_ap_parser = "=0.0.342"
line-index = "0.1.2"

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
version. `ra_ap_parser` must always use the identical rust-analyzer release.
Update both dependencies, the workspace MSRV, the tooling-reuse audit, and the
WASM parser-drop configuration together.

Package-level dependency boundaries:

```text
rustscript-core:
    ra_ap_syntax
    ra_ap_parser           only for LexedStr
    line-index
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

### Compiler-semantics reuse gate

Before implementing the subset checker, make a bounded spike using the matching
`ra_ap_base_db`, `ra_ap_hir`, `ra_ap_hir_ty`, and/or `ra_ap_ide_db` release.
Test whether a single in-memory Edition 2024 file can provide reliable local and
function resolution plus expression types while satisfying all of these:

- No filesystem, process, environment, networking, clock, or host-thread access
  on the core/WASM execution path.
- Successful `wasm32-unknown-unknown` compilation **and browser execution**.
- No unbounded background work or nondeterministic parallel execution.
- Acceptable release size/build time and a public API that can be isolated
  behind rustscript's private adapter.
- Diagnostics and resolution behavior sufficient for every required checker
  test without recreating a second checker beside it.

If the spike passes, use those APIs for resolution/type facts and keep only
rustscript-specific whitelist and execution checks. If it fails, do not ship a
half-integrated HIR database: record the exact crate graph, API/runtime blocker,
commands, and results in `docs/tooling-reuse.md`, then implement only the minimal
three-type checker specified here. The current release's transitive use of
database, channel, and parallel-runtime infrastructure must be evaluated rather
than assumed WASM-safe. This evidence gate is the exception path for custom
semantic code.

Do not add:

- `syn`, `prettyplease`, `proc_macro2`, or `quote`.
- `logos`, `chumsky`, `nom`, `winnow`, `pest`, or `lalrpop`.
- `tree-sitter` or `tree-sitter-rust`.
- Direct `rowan`, `text-size`, or rustc-lexer dependencies; use the types
  re-exported by `ra_ap_syntax`, `line-index`, and `ra_ap_parser::LexedStr`.
- `rustc_private`, `rustc_driver`, or other rustc internals.
- `clap`, `tokio`, `rayon`, or another framework or runtime.
- `anyhow` in library APIs.
- `rhai`, `rune`, `wasmtime`, `wasmer`, or another scripting engine.
- A second lexer, parser, syntax AST, formatter, syntax editor, line index, or
  diagnostic framework.
- Direct dependencies in `rustscript-core` or `rustscript-wasm` that require
  unavailable OS services at runtime. A required transitive crate may contain
  native or thread-related code only when the selected feature set compiles for
  `wasm32-unknown-unknown` and the WASM execution path does not invoke it.

All project-owned crate and fuzz-target source must use
`#![forbid(unsafe_code)]`. Transitive dependencies may contain unsafe internals
and must be reviewed as part of dependency selection; they are not covered by
the project-source prohibition.

## WASM requirements

The semantic engine must compile with:

```text
cargo check -p rustscript-wasm --target wasm32-unknown-unknown
```

The pinned `ra_ap_syntax` release has a native background-drop optimization
behind its recognized `no_salsa_async_drops` cfg. A default `Parse` drop can
otherwise attempt to create a thread, which is not a usable browser execution
path. Add and commit:

```toml
# .cargo/config.toml
[target.wasm32-unknown-unknown]
rustflags = ["--cfg", "no_salsa_async_drops"]
```

Treat this cfg as a version-coupled rust-analyzer integration detail: verify it
still exists and still selects synchronous dropping on every dependency update.
Add a WASM test that repeatedly parses, obtains errors/trees, and drops results;
a compile-only check is insufficient.

Add `wasm-bindgen-test` coverage for:

- Repeated `ra_ap_syntax` parse/error/tree/drop cycles.
- Valid parsing and checking.
- Interpreter execution.
- Exact output bytes.
- Structured diagnostics.
- Formatting.
- Fuel exhaustion.
- Output exhaustion.
- Deterministic repeated execution.
- Host-wrapper handling of an intentionally terminated test worker, including a
  structured `frontend-aborted` result and successful worker replacement.

Native-only dependencies must not appear in the `rustscript-wasm` target graph.

The browser package does not run rustc differential tests. Differential
conformance is established in native CI before publishing the WASM artifact.

`wasm32-unknown-unknown` aborts rather than unwinds on panic. Ship a small
JavaScript host wrapper that runs the WASM module in a dedicated Web Worker for
untrusted scripts. If the worker terminates unexpectedly, the wrapper must:

1. Report a generic structured `frontend-aborted` failure to the caller.
2. Discard the failed worker and module instance.
3. Start a clean worker before processing another request.

Document that this worker boundary contains a residual third-party parser abort;
the Rust WASM function itself cannot convert such an abort into a diagnostic.
Do not represent this limitation as successful parser recovery.

## Diagnostics

Core diagnostics are structured data. Terminal rendering belongs only to the
CLI.

Every diagnostic contains:

- Phase.
- Concise message.
- Optional byte span.
- Optional file name.
- Lazily calculated one-based line and column obtained from
  `line_index::LineIndex`.

The WASM adapter serializes this structure. The CLI renders it with
`annotate-snippets`. Do not compare diagnostic prose with rustc.

## Exact language profile

### Source and lexical rules

The CLI must read source as bytes. Invalid UTF-8 must produce a structured lexical diagnostic. The language accepts only ASCII source text.

Allowed whitespace is space, tab, carriage return, and line feed. Support `//` line comments, but reject block comments and documentation comments (`///` and `//!`).

Reject attributes, Unicode identifiers, raw identifiers, raw strings, byte strings, character literals, lifetimes, labels, non-decimal numeric bases, floating-point literals, and all unsupported punctuation.

Identifiers match:

```text
[A-Za-z_][A-Za-z0-9_]*
```

`_` by itself is forbidden. Keywords are valid only in their supported grammatical positions; they are never valid user-defined names. Reject these names in identifier positions:

```text
_
as async await break const continue crate dyn else enum extern false fn for if
impl in let loop match mod move mut pub ref return self Self static struct super
trait true type unsafe use where while

abstract become box do final gen macro override priv try typeof unsized virtual yield

macro_rules raw safe union
```

Also reserve `i64`, `bool`, `println`, and `main`. `main` is allowed only as the entry-point function name.

Use `ra_ap_parser::LexedStr`/`SyntaxKind` to distinguish `!`/`!=`, `=`/`==`, `<`/`<=`, `>`/`>=`, `-`/`->`, `&`/`&&`, and `|`/`||`. A single `&` or `|` is rejected by rustscript's lexical policy. Do not reproduce this tokenization with character lookahead.

### Literals and types

The only integer type is `i64`. Integer literals must be decimal digits followed immediately by `_i64`:

```text
DIGITS_i64
```

Examples: `0_i64`, `42_i64`, `999_i64`. Do not accept separators within the digit sequence. The positive component must fit in `i64`; unary `-` represents negative values. It is acceptable that `i64::MIN` cannot be written directly in version 0.

Also accept `true`, `false`, and `()`. There is no general string type. Recognize exactly the string token `"{}"` only for the supported print intrinsic; reject every other string literal.

The complete source-level type system is:

```rust
pub enum Type {
    I64,
    Bool,
    Unit,
}
```

The checker may use an internal `Never` type/control-flow summary, but `Never` is not source syntax.

### Functions

The only top-level item is a free function. There must be exactly one entry point with this exact signature:

```rust
fn main() {
    // ...
}
```

Rules:

- `main` has no parameters, no explicit return annotation, and normally completes with `()`.
- A missing return annotation on any other function means `()`; explicit `-> ()` is allowed for non-`main` functions and must round-trip.
- Function names are unique; there is no overloading.
- All function signatures are visible regardless of declaration order.
- Recursion and mutual recursion are supported by the language, although generated differential programs must use an acyclic call graph.
- Calling `main` from any function is forbidden.
- Parameters are immutable.
- Reject parameter and local names that collide with any function name.

### Statements

Support only:

```rust
let x = expression;
let x: i64 = expression;
let mut x = expression;
let mut x: bool = expression;
x = expression;
while condition { /* statements */ }
return;
return expression;
break;
continue;
println!("{}", expression);
expression;
```

Every `let` has an initializer. Assignment targets are bare local identifiers. There is no compound assignment. `break` and `continue` carry no values. `println!` is a statement, never an expression, and only the exact two-argument form shown above is accepted. Its expression must be `i64` or `bool`.

### Expressions

Support:

- `i64`, Boolean, and unit literals.
- Local variables.
- Calls whose target is a bare function identifier.
- Parenthesized expressions and block expressions.
- `if` expressions with mandatory `else` blocks.
- Unary `-` and `!`.
- `*`, `/`, `%`, `+`, `-`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, and `||`.

Do not support assignment expressions, ranges, closures, arrays, non-unit tuples, structs, `::` paths, methods, fields, indexes, casts, borrowing, dereferencing, `?`, `match`, `loop`, `for`, `if let`, `while let`, or `if` without `else`. There is no special `else if` syntax; nested `if` remains possible as the tail of an explicit `else { ... }` block.

Precedence, strongest to weakest:

1. Calls and parenthesized atoms.
2. Unary `-` and `!`.
3. `*`, `/`, `%`.
4. `+`, `-`.
5. `==`, `!=`, `<`, `<=`, `>`, `>=`.
6. `&&`.
7. `||`.

Arithmetic and logical operators are left-associative. Comparisons are non-associative: reject `1_i64 < 2_i64 < 3_i64`, while allowing `(1_i64 < 2_i64) == true` if it type-checks.

### Normative grammar

This grammar defines rustscript's admission policy and expected tree shapes; it
is **not** an instruction to implement a parser. Rust parsing, precedence,
associativity, delimiter handling, and recovery come exclusively from
`ra_ap_syntax`. The admission visitor maps the resulting typed AST to these
allowed forms and rejects everything else.

```text
program
    := function* EOF

function
    := "fn" IDENT "(" parameter_list? ")" ("->" type)? block

parameter_list
    := parameter ("," parameter)*

parameter
    := IDENT ":" type

type
    := "i64" | "bool" | "(" ")"

block
    := "{" statement* expression? "}"

statement
    := let_statement
     | assignment_statement
     | while_statement
     | return_statement
     | break_statement
     | continue_statement
     | println_statement
     | expression ";"

let_statement
    := "let" "mut"? IDENT (":" type)? "=" expression ";"

assignment_statement
    := IDENT "=" expression ";"

while_statement
    := "while" expression block

return_statement
    := "return" expression? ";"

break_statement
    := "break" ";"

continue_statement
    := "continue" ";"

println_statement
    := "println" "!" "(" FORMAT_PLACEHOLDER "," expression ")" ";"

expression
    := if_expression | logical_or_expression

if_expression
    := "if" expression block "else" block

logical_or_expression
    := logical_and_expression ("||" logical_and_expression)*

logical_and_expression
    := comparison_expression ("&&" comparison_expression)*

comparison_expression
    := additive_expression (comparison_operator additive_expression)?

additive_expression
    := multiplicative_expression (("+" | "-") multiplicative_expression)*

multiplicative_expression
    := unary_expression (("*" | "/" | "%") unary_expression)*

unary_expression
    := ("-" | "!") unary_expression | call_expression

call_expression
    := primary_expression ("(" argument_list? ")")?

argument_list
    := expression ("," expression)*

primary_expression
    := INTEGER
     | "true"
     | "false"
     | IDENT
     | "(" ")"
     | "(" expression ")"
     | block
```

Version 0 has no trailing commas. Validate the rust-analyzer block tree against
the statement/tail rules above; do not reproduce the old parser algorithm of
statement-starter dispatch, identifier lookahead, or expression parsing.

## Syntax ownership, checked IR, diagnostics, and defensive limits

`ParsedProgram` is an opaque owner of the validated source, rust-analyzer
`SourceFile`, and `LineIndex`. It is the only parse-stage program
representation. Do not copy it into a project-owned token stream or untyped
syntax AST.

Internally, use rust-analyzer `TextRange`/`TextSize` directly. Convert ranges at
public serialization and diagnostic boundaries to this inclusive-exclusive
type:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
```

Compute one-based line and column lazily through `LineIndex::try_line_col`; use
`LineIndex::line` for excerpts. Source bounds guarantee rust-analyzer's `u32`
text offsets are representable. Never rescan the source for newlines or
redundantly attach line/column to syntax or IR nodes.

Type checking traverses typed rust-analyzer nodes and lowers successful
programs directly into an opaque `CheckedProgram`. This checked executable IR
is allowed because the interpreter needs resolved bindings, resolved function
targets, types, mutability, control-flow structure, and runtime operations. It
must satisfy all of these constraints:

- It contains no tokens, trivia, comments, delimiters, parentheses, paths,
  parser recovery nodes, or precedence structure.
- Names use `ra_ap_syntax::SmolStr` internally or resolved numeric IDs; do not
  add another interning dependency without measured need.
- Resolved local/function IDs replace textual lookup wherever practical.
- Expressions carry their checked `Type` and source `TextRange`.
- Unary and binary operations reuse rust-analyzer's `ast::UnaryOp` and
  `ast::BinaryOp` families after admission has excluded unsupported variants.
- It retains only source distinctions required by canonical emission, such as
  local mutability/annotations and explicit function return annotations.
- `CheckedProgram` fields stay private, constructors are not public, and
  span-insensitive structural equality is available for project tests,
  generation, reduction, and differential comparison.

Do not introduce both a parsed AST and a checked AST. The only tree-shaped
project representation is the compact checked executable IR.

Use structured diagnostics:

```rust
pub enum Phase { Lex, Parse, Type, Runtime }

pub struct Diagnostic {
    pub phase: Phase,
    pub message: String,
    pub span: Option<Span>,
}
```

Human-readable diagnostics include the phase, optional file name, one-based line/column, source line, caret underline, and concise message. Do not imitate or compare rustc wording.

The frontend must consume the entire source, reject every rust-analyzer lexer or
parser error and every `ERROR` node/token, and enforce the subset grammar through
typed AST admission. Reject comparison chains and unsupported syntax at the
earliest useful rust-analyzer range. Project-owned byte validation, admission,
and checked-IR lowering must never panic or index beyond input on malformed
source. On unwind-capable native targets, contain dependency unwinds as
structured diagnostics; for untrusted browser input, use the required
replaceable Web Worker boundary.

Configurable defaults:

```text
Maximum source bytes:          1 MiB
Maximum tokens:                100,000
Maximum delimiter nesting:     256
Maximum syntax elements:       200,000
Maximum syntax/IR nesting:     256
Maximum functions:             1,024
Maximum parameters/function:   256
```

Reject unsupported syntax at the earliest useful span and avoid quadratic behavior on ordinary malformed input.

## Static type checking

Type checking is a separate subset-semantics pass over typed rust-analyzer AST
nodes. It is not a replacement Rust compiler: do not implement inference
variables, coercions, autoderef, traits, method lookup, generics, macro
expansion, borrow checking, const evaluation, or any other facility outside the
three-type profile. Record in `docs/tooling-reuse.md` why the pinned
rust-analyzer HIR/type-inference stack was not embedded (for example, public API
fit, WASM/runtime-service requirements, size, determinism, and resource-bound
integration), and revisit that decision when upgrading dependencies.

The pass has at least:

1. Global signature collection and validation.
2. Body checking plus direct checked-IR lowering after all valid signatures are
   known.

Use lexical scopes. Each binding records its type and mutability. Parameters are immutable; `let mut` is mutable; later `let` bindings may shadow earlier ones. Check an initializer before adding its new binding, so this refers to the outer `x`:

```rust
let x = 1_i64;
let x = x + 1_i64;
```

Assignment resolves the nearest binding, requires mutability, and preserves its exact type. Unknown names are errors.

Function calls require an existing non-`main` function, exact arity, and exact argument types. The call result is the declared return type.

Operator typing:

```text
-i64                         -> i64
!bool                        -> bool
i64 (+|-|*|/|%) i64         -> i64
i64 (<|<=|>|>=) i64         -> bool
i64 (==|!=) i64             -> bool
bool (==|!=) bool            -> bool
bool (&& | ||) bool          -> bool
```

Do not support equality on unit.

Control-flow rules:

- `if` conditions are Boolean and branches have compatible normal-completion types; an internally diverging branch is compatible with the other branch.
- `while` conditions are Boolean and the statement completes as unit.
- `break` and `continue` are legal only inside a loop.
- `return` is legal only inside a function and must match its declared return type; bare `return;` is legal only for unit.
- Semicolons discard expression values.
- A block without a tail normally completes as unit; a tail determines its normal-completion type.
- A function body’s normal-completion value must match the declared result. A body that cannot complete normally is valid when every exit satisfies the function result.
- Continue checking unreachable code; no warning is required.

Use an internal flow summary rather than exposing `Never` as a source type.

## Interpreter

Implement a direct tree-walking interpreter over a successfully checked program.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value { I64(i64), Bool(bool), Unit }
```

Use a global function table, a call stack, lexical scope stacks, and mutable/immutable runtime slots. All values are copied; there is no ownership, borrowing, heap, GC, or destruction model.

Use explicit control propagation equivalent to:

```rust
enum Control {
    Value(Value),
    Return(Value),
    Break,
    Continue,
}
```

`return`, `break`, and `continue` must propagate through nested blocks and conditionals correctly.

Match Rust evaluation order:

- Function arguments left to right.
- Binary operands left to right.
- Nested expressions before the surrounding operation.
- `&&` and `||` short-circuit.
- `let` initializer before installing its binding.
- Assignment RHS before updating its target.

Use checked `i64` operations (`checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem`, `checked_neg`). Return structured runtime errors for overflow, negation overflow, division/remainder by zero, `i64::MIN / -1`, and `i64::MIN % -1`. Never wrap.

The reusable interpreter owns `Vec<u8>` output and never writes directly to process stdout. `println!("{}", value)` formats `i64` in decimal or `bool` as `true`/`false`, followed by exactly `b'\n'`.

Runtime safeguards:

```rust
pub struct Limits {
    pub fuel: u64,
    pub maximum_call_depth: usize,
    pub maximum_output_bytes: usize,
}
```

Defaults:

```text
Fuel:                   1,000,000 evaluation steps
Maximum call depth:     1,024
Maximum output bytes:   1 MiB
```

Every statement and expression consumes fuel. Return structured errors for fuel, call-depth, and output exhaustion.

## Canonical formatting and public API

The minimal canonical subset emitter walks admitted typed rust-analyzer nodes
(or the checked IR when emitting generated programs). It must deterministically
emit valid Rust 2024 subset syntax, use stable indentation, print every integer
with `_i64`, preserve local mutability/annotations and explicit function return
annotations, emit only the exact print form, and conservatively parenthesize
every composite unary/binary expression. It may intentionally discard comments
and original whitespace. Repeated formatting is byte-identical.

Do not create a general pretty-print document algebra, tokenize source again,
or reimplement Rust precedence. Use rust-analyzer operator enums and AST accessors
to select supported output. This emitter exists only because
`ra_ap_syntax` does not provide a stable general formatter.

Required properties:

```text
canonical = format_program(parsed)
format_program(parse(canonical)) == canonical
normalize(check(parse(canonical))) == normalize(check(parsed)) // when both check
```

Expose a small API from `rustscript-core`:

```rust
pub fn parse(source: &str, limits: ParseLimits) -> Result<ParsedProgram, Diagnostic>;

// Used by the CLI and byte fuzz target to reject invalid UTF-8 before frontend validation.
pub fn parse_bytes(source: &[u8], limits: ParseLimits) -> Result<ParsedProgram, Diagnostic>;

pub fn check(program: &ParsedProgram) -> Result<CheckedProgram, Vec<Diagnostic>>;

pub fn run(program: &CheckedProgram, limits: Limits)
    -> Result<RunResult, RuntimeDiagnostic>;

pub fn format_program(program: &ParsedProgram) -> String;

// Rust-analyzer syntax debug output for the pinned dependency version.
pub fn debug_syntax(program: &ParsedProgram) -> String;

pub struct RunResult {
    pub stdout: Vec<u8>,
    pub steps: u64,
}
```

`ParsedProgram` and `CheckedProgram` are opaque. Do not expose rust-analyzer
types, mutable checker/interpreter internals, or a public constructor for an
invalid checked program.

## CLI

Implement:

```text
rustscript check FILE
rustscript run FILE
rustscript ast FILE
rustscript fmt FILE
```

- `check`: parse and type-check; no stdout on success.
- `run`: parse, check, and interpret; stdout contains only exact script output.
- `ast`: print the pinned rust-analyzer syntax tree through `debug_syntax`; do
  not maintain a duplicate debug AST. Snapshot stability is guaranteed for the
  committed lockfile, and dependency upgrades may intentionally update it.
- `fmt`: print canonical Rust source to stdout without modifying the file.
- All diagnostics go to stderr.
- Exit `0` on success, `1` for source/type/runtime errors, and `2` for usage errors.
- Use `std::env::args_os` or an equally small solution; no large CLI framework is needed.

## Required examples

Include these programs and assert exact stdout bytes.

### Arithmetic — `b"42\n"`

```rust
fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn main() {
    println!("{}", add(20_i64, 22_i64));
}
```

### Evaluation order — `b"1\n2\n30\n"`

```rust
fn left() -> i64 {
    println!("{}", 1_i64);
    10_i64
}

fn right() -> i64 {
    println!("{}", 2_i64);
    20_i64
}

fn main() {
    println!("{}", left() + right());
}
```

### Short circuit — `b"false\ntrue\n"`

```rust
fn mark() -> bool {
    println!("{}", 99_i64);
    true
}

fn main() {
    println!("{}", false && mark());
    println!("{}", true || mark());
}
```

`99` must not be printed.

### Sum loop — `b"55\n"`

```rust
fn sum_to(n: i64) -> i64 {
    let mut index: i64 = 0_i64;
    let mut total: i64 = 0_i64;

    while index <= n {
        total = total + index;
        index = index + 1_i64;
    }

    total
}

fn main() {
    println!("{}", sum_to(10_i64));
}
```

### Euclidean GCD — `b"6\n"`

```rust
fn gcd(a: i64, b: i64) -> i64 {
    let mut x: i64 = a;
    let mut y: i64 = b;

    while y != 0_i64 {
        let remainder: i64 = x % y;
        x = y;
        y = remainder;
    }

    x
}

fn main() {
    println!("{}", gcd(84_i64, 30_i64));
}
```

### Shadowing — `b"2\n"`

```rust
fn main() {
    let x: i64 = 1_i64;
    let x: i64 = x + 1_i64;
    println!("{}", x);
}
```

## Required rejection corpus

Add deterministic tests proving rejection of:

```rust
// Type mismatch
fn main() { let value: bool = 1_i64; }

// Assignment to immutable local
fn main() { let value: i64 = 1_i64; value = 2_i64; }

// Unknown variable
fn main() { println!("{}", missing); }

// Invalid arity
fn identity(value: i64) -> i64 { value }
fn main() { println!("{}", identity()); }

// Reference
fn main() { let value: i64 = 1_i64; let reference = &value; }

// Generic
fn identity<T>(value: T) -> T { value }
fn main() {}

// Heap type and path
fn main() { let text = String::new(); }

// Unsupported formatting
fn main() { println!("{:?}", 1_i64); }

// Comparison chain
fn main() { println!("{}", 1_i64 < 2_i64 < 3_i64); }

// Missing else
fn main() { let value: i64 = if true { 1_i64 }; }
```

## Testing requirements

### Rust-analyzer frontend and admission tests

Use table-driven tests covering `LexedStr` error/range propagation, empty
blocks, unit, functions, parameter lists, explicit/implicit unit returns,
immutable/mutable lets, annotations, assignment, shadowing syntax,
zero/one/multiple-argument calls, nested blocks, tails, semicolon-discarded
expressions, `if`, nested `if`, `while`, `return`, `break`, `continue`,
`println!`, every operator, precedence, associativity, short-circuit AST
structure, comments, EOF, unterminated blocks, invalid tokens/suffixes/integers/
strings, single `&`/`|`, comparison chains, size/token/node/depth limits,
non-ASCII, and invalid UTF-8 through the byte API.

Assert the actual rust-analyzer tree and operator APIs rather than a duplicate
parser AST. Include explicit assertions equivalent to:

```text
1_i64 + 2_i64 * 3_i64
=> outer ast::BinExpr op_kind() == Add
=> outer rhs is ast::BinExpr with op_kind() == Mul

false || true && false
=> outer ast::BinExpr op_kind() == LogicOp::Or
=> outer rhs is ast::BinExpr with op_kind() == LogicOp::And
```

Add focused admission tests proving that every supported construct is accepted
through typed AST accessors and representative unsupported `ast::Item`,
`ast::Stmt`, `ast::Expr`, `ast::Type`, and `ast::Pat` variants are rejected.
Add a dependency-boundary regression proving `LexedStr` and
`ra_ap_syntax` use the same pinned edition/release.

### Type-checker tests

Cover correct/wrong function returns, missing tails, semicolon-to-unit behavior, valid/invalid calls, wrong argument type/arity, duplicate functions, missing/duplicate/invalid `main`, immutable/mutable assignment, lexical scopes, shadowing, initializer-before-shadow, unknown names, operator errors, comparison errors, branch mismatch, non-Boolean conditions, `break`/`continue` outside loops, return mismatch, nested early return, recursive signatures, calling `main`, name collisions, and printing unit.

### Interpreter tests

Cover every operator, negative values, exact Boolean formatting, precedence, left-to-right evaluation, short-circuiting, function/nested calls, scopes, shadowing, mutation, loops, `break`, `continue`, nested control propagation, early return, safe bounded recursion, all arithmetic traps, fuel/call/output exhaustion, deterministic step counts, and raw `Vec<u8>` output equality without trimming.

### Regression corpus

Include deterministic regressions for unary versus binary minus, nested parentheses, tail versus statement expressions, initializer-before-shadow, left-to-right calls, short-circuiting, non-associative comparisons, return and continue propagation through nested blocks, negative division/remainder, empty output, and multiple output lines. Every future fuzz/differential failure must be minimized and added to this persistent corpus with a short bug-category note.

## Differential testing against rustc

This is a primary deliverable. Keep two complementary strategies separate:

1. Coverage-guided byte fuzzing for parser robustness.
2. Grammar-aware, seed-based property/differential testing for semantic equivalence.

Never invoke `rustc` from a libFuzzer target.

### Rustc oracle and subprocess safety

Implement `RustcOracle`. Resolve the compiler in this order:

1. Explicit `--rustc PATH`.
2. `RUSTC` environment variable.
3. `rustc` on `PATH`.

Record `rustc -Vv` in every failure artifact. Invoke the compiler directly with `std::process::Command`, never a shell, using arguments equivalent to:

```text
rustc
    --edition=2024
    --crate-type=bin
    -C overflow-checks=yes
    -C panic=abort
    -C debuginfo=0
    -A warnings
    --color=never
    SOURCE.rs
    -o EXECUTABLE
```

Use a fresh temporary directory and the platform executable suffix. Default compile timeout is 10 seconds; run timeout is 2 seconds. Set `RUST_BACKTRACE=0` on the native process.

Spawn with piped stdout/stderr, drain both concurrently so a full pipe cannot deadlock, enforce independent output caps, and on timeout kill **and reap** the child. Capture compiler status/stdout/stderr and native status/stdout/stderr separately.

### Comparison rules

For every generated program that parses, checks, and completes in the interpreter without a trap or safeguard error:

- Formatting then reparsing and checking must preserve the normalized checked
  executable IR.
- Checking and running the reparsed canonical program must succeed.
- Two interpreter runs with identical limits must have identical stdout, result category, and step count.
- rustc compilation must succeed.
- Native execution must succeed.
- Native stdout must exactly equal interpreter stdout.
- Native stderr must be empty.

Never report a mismatch merely because rustc accepts syntax outside the subset, and never compare rustc diagnostic prose.

Represent failures structurally, including at least seed, case index, source, all captured streams, rustc version, and a reason:

```rust
pub enum DiffFailureKind {
    PrettyPrintRoundTrip,
    RustcRejectedAcceptedProgram,
    NativeTimeout,
    NativeFailure,
    StdoutMismatch,
    UnexpectedNativeStderr,
    NondeterministicInterpreter,
}
```

The main generator produces only successfully terminating, nontrapping programs. Add an optional small trap suite for overflow and division/remainder by zero. For traps, compare outcome categories rather than panic text, print nothing before the trap, require native failure, and require the corresponding interpreter error.

### Typed generator

Generate a valid typed, syntax-neutral program specification that maps directly
to the checked executable IR, not blind source strings and not a second parsed
Rust AST. Materialize Rust syntax with the shared canonical subset emitter.
Where rust-analyzer construction APIs cover an operation cleanly, prefer
`ast::make`/`SyntaxFactory`; never add a parallel general syntax builder. Carry
context equivalent to:

```rust
struct GenerationContext {
    locals: Vec<GeneratedBinding>,
    functions: Vec<FunctionSignature>,
    expected_type: Type,
    expression_depth: usize,
    statement_budget: usize,
    loop_depth: usize,
}
```

Use deterministic names (`f0`, `f1`, `x0`, `x1`, ...). Generate zero to four helpers plus `main`, zero to three parameters, zero to eight statements per block, expression/block depth at most five, loop nesting at most two, loop bounds at most eight, and at most 64 output lines.

Build a directed acyclic helper call graph: each helper may call only earlier helpers, while `main` may call any helper. The language itself still supports recursion.

Generate expressions from an expected type:

- `i64`: small constants, matching locals/calls, unary negation, arithmetic, typed `if`, typed blocks.
- `bool`: constants, matching locals/calls, `!`, comparisons/equality, `&&`, `||`, typed `if`, typed blocks.
- unit: `()`, unit blocks, or unit-returning calls.

Generate arguments from signatures. Prefer literal, provably nonzero denominators. Keep magnitudes in roughly `-1000..=1000`. Use the interpreter only as a final suitability filter; still run rustc as the independent oracle.

Generate loops in a canonical terminating form with a protected counter and an unavoidable increment. Initially omit generated `continue`, or ensure every `continue` occurs only after the increment. Generate `break` only in bounded forms.

### Runner, reproduction, artifacts, and shrinking

Support:

```text
rustscript-difftest --seed 1 --cases 1000
rustscript-difftest --seed 12345 --case 87
rustscript-difftest --replay path/to/failure.rs
rustscript-difftest --rustc /path/to/rustc
rustscript-difftest --keep-all
```

On failure, create a non-overwriting directory like:

```text
artifacts/differential/seed-12345-case-87/
├── failure.rs
├── canonical.rs
├── minimized.rs
├── ast.txt
├── metadata.txt
├── interpreter.stdout
├── native.stdout
├── native.stderr
├── compiler.stdout
└── compiler.stderr
```

Metadata includes seed, case index, category, rustc version, exact rustc argv, interpreter/generator limits, host OS/architecture, and an exact reproduction command.

Use proptest shrinking where practical and add a best-effort syntax/checked-IR
reducer that preserves the original failure category. Prefer
`SyntaxEditor`/typed rust-analyzer nodes for reductions over parsed source and
the shared typed specification/checked IR for generated cases. Try removing
unused functions/statements/prints, simplifying bodies, replacing expressions
with same-typed literals or legal children, shrinking integers and loop bounds,
simplifying conditions/branches, reducing arguments, and inlining simple
generated locals. Do not implement another parser or generic tree editor.

Start with one program per rustc invocation. Optional batching may be added only after correctness is established; any batch failure must be isolated and reproduced as a standalone canonical program before artifact capture and shrinking.

### Stable differential smoke tests

Normal `cargo test` must run a deterministic fixed-seed corpus of 25–50 generated programs through the installed rustc, using one worker by default. Compare exact bytes. The standalone runner may support bounded parallel compilation and an optional source-hash cache.

## Property and fuzz testing

Use bounded `proptest` cases under normal `cargo test` for:

- Byte preflight and `LexedStr` policy termination on arbitrary ASCII.
- Rust-analyzer parse/admission termination on arbitrary ASCII.
- Canonical checked-IR round trip.
- Canonical emitter idempotence.
- Deterministic interpretation, including step counts and error categories.
- rustc acceptance of generated checked programs.
- Exact native/interpreter stdout equivalence.

Create two `cargo-fuzz` targets:

### `parser_bytes`

Feed arbitrary bytes through the byte-oriented parse entry point. It must reject
invalid UTF-8/non-ASCII, never hang, and respect limits. For bounded UTF-8
inputs, also call `ra_ap_syntax::fuzz::check_parser` so rust-analyzer's own tree
invariants are exercised; a panic there is a dependency fuzz finding. On
successful rustscript admission, format and reparse; require canonical
idempotence. If checking succeeds, compare normalized checked IR and run with
tight limits without panic.

### `ast_roundtrip`

Use `arbitrary` or a bounded byte decision stream to build the shared typed
program specification/checked IR, then require:

```text
typed specification -> canonical emit -> parse -> check -> normalized checked-IR equality
```

Also interpret without panic. Put shared normalization/generation support in an
explicit core `generator-support` feature or a dedicated support crate; do not
copy the IR into the fuzz package. Neither fuzz target may spawn rustc.

## README requirements

Document the purpose, exact syntax, strict-subset invariant, unsupported Rust
features, arithmetic policy, safeguards, rust-analyzer frontend and reuse map,
checked-IR/checker/interpreter architecture, the canonical-emitter exception,
fixed `println!` intrinsic rationale, one-way conformance rationale, exact
stdout comparison, non-comparison of diagnostics, WASM parser-drop cfg and
worker-isolation boundary, examples, tests, differential runner, artifact
replay/minimization, and fuzzing. Include `docs/tooling-reuse.md` and link it
from the README.

Include working commands:

```text
cargo build --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

cargo check -p rustscript-wasm --target wasm32-unknown-unknown
wasm-pack test --node crates/rustscript-wasm

cargo run -p rustscript-cli -- check examples/gcd.rs
cargo run -p rustscript-cli -- run examples/gcd.rs
cargo run -p rustscript-cli -- ast examples/gcd.rs
cargo run -p rustscript-cli -- fmt examples/gcd.rs

cargo run -p rustscript-difftest -- --seed 1 --cases 1000

cargo +nightly fuzz build parser_bytes
cargo +nightly fuzz build ast_roundtrip
cargo +nightly fuzz run parser_bytes
cargo +nightly fuzz run ast_roundtrip
cargo +nightly fuzz tmin parser_bytes PATH_TO_CRASH
```

Explain that stable property tests and the standalone differential runner remain available when `cargo-fuzz` is unavailable.

## Implementation order

Implement and keep compiling in this order:

1. Workspace, crate boundaries, exact rust-analyzer pins, `.cargo/config.toml`,
   spans, diagnostics, limits, dependency policy, and initial
   `docs/tooling-reuse.md`.
2. Bounded byte validation followed by `ra_ap_parser::LexedStr` lexical policy,
   token counting, ranges, and delimiter limits.
3. `ra_ap_syntax::SourceFile::parse`, full error rejection, typed-AST subset
   admission, and syntax-element/depth limits.
4. Rust-analyzer frontend/admission tests, including operator tree assertions
   and native/WASM parse-drop tests.
5. Two-pass subset checker with direct checked-IR lowering and checker tests.
6. Minimal canonical subset emitter and checked-IR round-trip properties.
7. Interpreter and interpreter tests.
8. CLI, WASM adapter, worker wrapper, and examples.
9. Safe rustc subprocess layer and oracle.
10. Shared typed program specification/generator and deterministic smoke corpus.
11. Standalone differential runner and reproduction.
12. Artifacts and rust-analyzer syntax/checked-IR-aware shrinking.
13. Fuzz targets, rust-analyzer parser invariants, and regression corpus.
14. README, final tooling-reuse/dependency audit, strict lint cleanup, and full
    verification.

Make focused commits for logical milestones so the existing draft PR remains reviewable commit by commit.

## Completion checklist

- [ ] All included valid examples parse, type-check, interpret, and compile with the configured rustc.
- [ ] Interpreter/native stdout matches byte-for-byte for every valid example.
- [ ] Every required invalid example is rejected with a structured diagnostic and useful span.
- [ ] `docs/tooling-reuse.md` maps every syntax-adjacent operation to the reused
      rust-analyzer/tooling API or documents the tested reason for the minimal
      custom exception.
- [ ] No project lexer, token model, parser cursor/grammar, syntax AST, line
      index, operator parser, precedence table, or generic syntax editor exists.
- [ ] `ra_ap_parser::LexedStr` and `ra_ap_syntax` are exact-version pinned in
      lockstep; parsing occurs only through `SourceFile::parse` in Edition 2024.
- [ ] Typed `ast::*` variants and traits drive admission; unsupported variants
      are rejected exhaustively rather than skipped.
- [ ] Project-owned byte validation, token policy, admission, and lowering code
      does not panic, hang, or index unsafely on user input.
- [ ] Native parser dependency unwinds become structured diagnostics; browser parsing of untrusted input is isolated in a replaceable Web Worker and the residual abort limitation is documented.
- [ ] A browser integration test verifies structured worker-failure reporting and clean worker replacement.
- [ ] WASM tests repeatedly parse and drop rust-analyzer results with the pinned
      `no_salsa_async_drops` target cfg; dependency upgrades revalidate it.
- [ ] Canonical emission round-trips through parse/check to equivalent normalized
      checked IR and is idempotent.
- [ ] Precedence and associativity come from the rust-analyzer parse tree and
      operator APIs and match Rust for the subset.
- [ ] Arguments and operands evaluate left to right; `&&`/`||` short-circuit.
- [ ] Checked arithmetic matches native Rust with overflow checks.
- [ ] Normal tests include a deterministic 25–50 case rustc differential corpus.
- [ ] The standalone runner reproduces by seed/case and replay file.
- [ ] Failures save complete, non-overwriting artifacts and a best-effort minimized case.
- [ ] Both fuzz targets build.
- [ ] Every crate obeys the dependency allowlist and the WASM graph contains no runtime use of unavailable OS services.
- [ ] `rustscript-wasm` builds for `wasm32-unknown-unknown` and its WASM tests pass.
- [ ] Every project-owned crate and fuzz target forbids unsafe code.
- [ ] No production `todo!()`, `unimplemented!()`, swallowed errors, shell
      commands, second frontend implementation, or unstable rustc-internal
      crates.
- [ ] Public APIs are documented and important invariants are explained.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.

Before declaring completion, review the full diff against `main`, run the complete verification suite, and perform an independent full-diff review. Address every confirmed critical/high-severity finding and all in-scope lower-severity findings.

In the final handoff, report:

- Workspace structure created.
- Exact commands executed and their results.
- Unit/property/integration test results.
- Differential seed and case count.
- Which fuzz targets were built and which, if any, were actually run.
- Independent-review findings and resolutions.
- Known limitations or blockers.

Do not claim a fuzz target or long differential run was executed unless it actually was.

