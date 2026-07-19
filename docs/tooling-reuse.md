# Rust tooling reuse audit

The frontend keeps rust-analyzer's lossless syntax tree until checking has
resolved names and lowered directly to compact executable IR. There is no
project token model, parser cursor, syntax AST, precedence table, or line index.

| Need | Reused API | Rustscript-specific policy |
| --- | --- | --- |
| Rust token boundaries and errors | `ra_ap_parser::LexedStr` in `Edition2024` | Bounded whitelist over lexer-provided kinds, text, and ranges |
| Rust 2024 parsing | `ra_ap_syntax::SourceFile::parse` | Native unwind containment and strict rejection of parser errors/recovery nodes |
| Parser validation | `Parse::errors`, `SyntaxKind::ERROR` | Stable diagnostic conversion |
| Typed syntax admission | `ast::Item`, `ast::Stmt`, `ast::Expr`, `ast::Type`, `ast::Pat`, and AST traits | Exhaustive strict-subset admission |
| Statement boundaries | `ast::Stmt` variants and statement token ownership | Reject a direct `SEMICOLON` with no `LetStmt`/`ExprStmt` owner because rust-analyzer intentionally exposes no empty-statement AST node |
| Operator identity and precedence | `PrefixExpr::op_kind`, `BinExpr::op_kind`, and rust-analyzer operator enums/tree shape | Three-type operator rules; no precedence implementation |
| Macro envelope | `MacroCall` path accessors and `TokenTree::token_trees_and_tokens` | Exact `println!("{}", expression)` policy |
| Macro value expression | A bounded wrapper parsed only by `SourceFile::parse` | Required because macro token trees are opaque; no token-tree expression parser is implemented |
| Source ranges | `TextRange` and `TextSize` | Conversion to public inclusive-exclusive `Span` only at diagnostics/API boundaries |
| Lines and columns | `line_index::LineIndex` | One-based public location conversion |
| Syntax limits | `preorder_with_tokens` and `WalkEvent` | Element/depth counters and configured bounds |
| Syntax debug output | Alternate `Debug` for the pinned rust-analyzer syntax node | Public wrapper keeps rust-analyzer types private |
| Parse-stage canonical output | Typed rust-analyzer `ast::*` nodes and operator enums | Small subset emitter for `format_program`; discards trivia and emits only admitted forms |
| Checked/generated canonical output | Checked executable IR plus rust-analyzer operator enums retained during lowering | Small checked-IR emitter for generation, reduction, and differential tests |
| Generated/reduced programs | Checked executable IR plus the shared emitter | Typed deterministic decisions and type-preserving reduction candidates |

The macro-expression wrapper is the only extra parse. Its envelope is first
validated from rust-analyzer token-tree elements; its bounded contents are then
embedded in a fixed function/`let` wrapper and parsed through
`SourceFile::parse`. This avoids the prohibited `TopEntryPoint` macro parser and
avoids a handwritten expression grammar. Wrapper ranges are mapped back to the
original token-tree expression range before checked IR is retained.

## Compiler-semantics reuse gate

The matching rust-analyzer semantic stack was evaluated on 2026-07-18 with an
ephemeral crate containing exact `=0.0.342` dependencies on
`ra_ap_base_db`, `ra_ap_hir`, and `ra_ap_hir_ty`:

```text
cargo info ra_ap_base_db@0.0.342
cargo info ra_ap_hir@0.0.342
cargo info ra_ap_hir_ty@0.0.342
cargo tree --target wasm32-unknown-unknown
cargo metadata --format-version 1 --filter-platform wasm32-unknown-unknown
RUSTFLAGS='--cfg no_salsa_async_drops' cargo check --target wasm32-unknown-unknown
```

The dependency-only WASM check succeeded in 14.38 seconds, so compilation alone
is not the blocker. The locked reproduction resolved 137 packages; the
[complete package/version and normal/build edge tree](hir-spike-tree.md) is
committed with its exact manifest inputs and commands. Inspection of the
published primary crate manifests and resulting graph found:

- `ra_ap_base_db` and `ra_ap_hir_ty` enable Salsa's `rayon` and
  `salsa_unstable` features;
- the target graph includes Rayon, Crossbeam queues/channels, `jod-thread`,
  `thread_local`, DashMap, parking-lot synchronization, VFS/path infrastructure,
  tracing subscribers, and rustc trait-solver crates;
- the concrete `RootDatabase` needed by the documented `Semantics` examples is
  supplied by the additional `ra_ap_ide_db` crate, not by the three semantic
  crates;
- the first concrete API blocker was constructing an in-memory `RootDatabase`
  for one Edition 2024 file without bringing in IDE/VFS/crate-graph and
  proc-macro configuration state. The attempted shape was:

```rust
let source = r#"fn main() { let value = 1_i64; value; }"#;
let parse = ra_ap_syntax::SourceFile::parse(source, Edition::Edition2024);
// Desired next step: Semantics::type_of_expr on the local path expression.
// Blocker: no small public constructor exists for the required database/input
// state in ra_ap_hir/ra_ap_hir_ty alone.
```

- a single file must therefore still be installed into VFS/source-root/crate
  graph state before `Semantics::type_of_expr` can resolve locals and calls;
- a link-only WASM build does not establish browser execution, absence of
  background/thread paths, deterministic bounded work, or sufficient
  diagnostics for every subset rule.

Because the small in-memory API path failed before a runnable semantic adapter
existed, the HIR spike did not proceed to browser execution. The currently
shipped frontend path is browser-tested through `wasm-pack test --headless
--firefox` and the packaged Worker is tested after `wasm-pack build` by
`npm --prefix crates/rustscript-wasm test`. Shipping the HIR stack would add a
large database/runtime beside the subset checks rather than replace them.
Rustscript consequently implements only the permitted lexical-scope checker for
`i64`, `bool`, and `()`. Repeat the HIR measurement, including browser
execution if the API blocker is removed, on every rust-analyzer upgrade.

## Version-coupled WASM detail

`ra_ap_syntax = 0.0.342` recognizes `no_salsa_async_drops`. The committed
`.cargo/config.toml` sets it only for `wasm32-unknown-unknown`, selecting
synchronous parse-result dropping. WASM tests repeatedly parse valid/invalid
programs, obtain syntax/error results, and drop them. The browser Worker
recovery test imports the production `host.js` through the `wasm-bindgen-test`
browser server and uses real browser Workers to force an abort, observe
`frontend-aborted`, and verify that the host terminates and replaces the failed
Worker. The production `worker.js` and generated WASM module are covered by the
Node Worker integration test. The direct `wasm-bindgen-futures` dev-dependency
is a documented test-only exception: `wasm-bindgen` expands the asynchronous
imported JavaScript test through that crate, while no production adapter path
uses it. Revalidate the cfg in the dependency source and rerun the browser/Node
tests whenever the exact rust-analyzer pins change.
