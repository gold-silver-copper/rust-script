# Rust tooling reuse audit

| Need | Reused API | Rustscript-specific code |
| --- | --- | --- |
| Rust token boundaries | `ra_ap_parser::LexedStr` | Bounded subset policy over token kinds/text |
| Rust 2024 parsing | `ra_ap_syntax::SourceFile::parse` | Panic containment on unwind targets |
| Parser errors | `Parse::errors` and `SyntaxKind::ERROR` | Stable diagnostic conversion |
| Typed syntax | `ra_ap_syntax::ast` and `AstNode` | Strict subset admission and checked-IR lowering |
| Operators | Typed AST `op_kind` APIs | Subset type rules only |
| Source ranges | `TextRange` and `TextSize` | Public `Span` conversion |
| Lines and columns | `line_index::LineIndex` | One-based public location conversion |
| Canonical output | No stable general Rust formatter fits the WASM/resource contract | Small emitter over checked IR only |

## Compiler-semantics reuse gate

The matching rust-analyzer HIR stack is not embedded. Its database/channel and
parallel-runtime transitive architecture is substantially larger than the
syntax crates, does not provide a small standalone three-type checker API, and
would require host-oriented runtime services incompatible with the bounded
browser core. The project therefore uses the issue's permitted minimal checker.
This decision must be revisited with every rust-analyzer dependency upgrade.
