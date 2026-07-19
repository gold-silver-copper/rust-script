# Regression corpus

Every minimized fuzz or differential finding should be added here with a short
bug-category note and, when safe, a deterministic test. Inputs that intentionally
panic a dependency invariant are documented here instead of being placed in the
active fuzz corpus.

| Category | Input or scenario | Persistent coverage |
| --- | --- | --- |
| Unary versus binary minus | `let value = -1_i64 - 2_i64;` | `supported_profile_constructs_check`; arithmetic trap tests |
| Nested parentheses and tails | `println!("{}", 1_i64 + (2_i64 / 0_i64));` | `println_nested_runtime_spans_map_to_original_source` |
| Tail versus statement expression | `format_program_canonicalizes_parsed_but_type_invalid_sources` sample | Canonical parsed-program round-trip test |
| Initializer-before-shadow | `let x = 1_i64; let x = x + 1_i64;` | Normative `shadowing` example |
| Left-to-right calls | `left() + right()` | Normative `evaluation_order` example and difftest |
| Short-circuiting | `false && mark()`, `true || mark()` | Normative `short_circuit` example and typechecker regressions |
| Non-associative comparisons | `1_i64 < 2_i64 < 3_i64` | Required rejection corpus |
| Nested return/continue propagation | Early return and bounded loop samples | `supported_profile_constructs_check` and typechecker rejection corpus |
| Negative division/remainder | `gcd` and trap-suite signed cases | Difftest trap suite and interpreter trap tests |
| Empty output and multiple output lines | `fn main() {}` and examples with several prints | Core examples and difftest example oracle tests |
| Macro wrapper injection | `println!("{}", 1_i64;)` and extra statement variants | `rejects_profile_edge_cases` |
| Nested macro diagnostic span mapping | `println!("{}", || true)` | `println_nested_admission_spans_map_to_original_source` |
| Nested macro runtime span mapping | `println!("{}", 1_i64 + (2_i64 / 0_i64))` | `println_nested_runtime_spans_map_to_original_source` |
| Flow-sensitive Rust subset soundness | nested `return`, short-circuit RHS divergence, final `while` tails | `rejects_profile_edge_cases` |
| Recursive unreachable locals | thousands of unreachable locals after recursive call | `unreachable_locals_do_not_preallocate_every_recursive_frame` |
| Cross-helper arithmetic growth | four helper layers each multiply an earlier result by `9_i64` five times | `overflowing_helper_chain_uses_the_safe_suitability_fallback` and the evaluator-backed generator filter |
| Concurrent oracle timeouts | cloned oracles enter `wait_timeout` together on Unix | process-wide wait serialization and `serializes_concurrent_timeout_waits` |
| Dependency parser invariant | `{#}` reproduces a pinned `ra_ap_syntax::fuzz::check_parser` invariant panic for `0.0.342` | Documented dependency finding; product byte/token policy rejects `#` before parsing |

Known dependency finding reproduction:

```text
cargo +nightly fuzz run parser_bytes fuzz/regressions/parser_bytes/check_parser_panic_d2c3522.txt
input bytes: {#}
observed result: dependency invariant panic inside ra_ap_syntax::fuzz::check_parser
rustscript product path: rejected as unsupported token `#` before SourceFile::parse
```
