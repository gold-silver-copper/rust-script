# Agent operating manual

This repository is `rustscript`: a resource-bounded interpreter for a strict
subset of Rust 2024, differentially tested against native `rustc`. The
authoritative specification is vendored at [`docs/spec.md`](docs/spec.md)
(the implementation prompt of GitHub issue #2). The spec is a contract, not a
suggestion: its priority order controls whenever goals conflict.

## Session workflow

Every working session — including one started with nothing more than
"continue" — follows this loop:

1. Read [`docs/backlog.md`](docs/backlog.md) and pick the highest-priority
   open item you can complete and verify in this session. Prefer finishing
   one item over starting three.
2. Implement it with tests. Every behavior change gets a test; every bug fix
   gets a regression test first.
3. Run `scripts/verify.sh` (use `FULL=1 scripts/verify.sh` when wasm-pack,
   npm, Firefox, and nightly are available). Do not conclude a session red.
4. If any fuzz or differential failure appeared, minimize it and add it to
   the persistent regression corpus with a category note in
   `docs/regression-corpus.md`.
5. Update `docs/backlog.md`: mark the item done (move it to the log at the
   bottom with the commit hash), and append any new items you discovered.
6. Commit in focused, logical commits; push; confirm CI is green before
   ending the session.

## What "better" means, ranked

When choosing work, this is the order. Never trade a higher rank for a lower
one.

1. **Correctness of the invariant** — every accepted program compiles with
   `rustc --edition=2024` and produces byte-identical stdout under overflow
   checks. A differential mismatch outranks everything.
2. **Robustness under hostile input** — no panic, abort, hang, or unbounded
   resource use on any byte sequence, native or wasm. Stack-overflow aborts
   inside `SourceFile::parse` have happened twice (see
   `docs/regression-corpus.md`); treat parser recursion as untrusted.
3. **Spec conformance** — anything in `docs/spec.md` not yet fully met.
4. **Verification depth** — more fuzzing (longer campaigns, corpus growth),
   more differential volume (new seeds), stronger property tests, boundary
   tests, coverage of untested paths.
5. **Documentation truthfulness** — docs must describe what the code does
   today; fix drift in whichever direction is correct.
6. **Performance and code size** — only with a measurement showing the win,
   and never at the cost of ranks 1–5.

Out of scope without explicit human direction: expanding the language subset
(new types, constructs, or intrinsics), changing the public API surface,
adding dependencies, or relaxing any limit. Propose these in the backlog's
"Needs human decision" section instead of implementing them.

**Subset growth is governed by the HIR-reuse gate**
([`docs/hir-reuse-gate.md`](docs/hir-reuse-gate.md)). Value-semantics features
(more scalars, `for`, `match`, compound assignment, value tuples/arrays) may be
added on the hand-rolled checker. References/borrows, generics, and traits may
**not** be implemented until that gate is re-run and passes — do not grow a
hand-written borrow checker or trait solver beside rust-analyzer. Re-run the
gate on every `ra_ap_*` pin bump.

## Hard invariants (never violate)

- The one-way compatibility contract and exact-stdout comparison
  (spec, "Compatibility contract").
- The rust-analyzer reuse mandate: no project lexer, token model, parser,
  precedence table, syntax AST, line index, or general formatter.
  `ra_ap_syntax`/`ra_ap_parser` stay exact-version pinned in lockstep.
  `docs/tooling-reuse.md` is updated with any syntax-adjacent change.
- The dependency blocklist in the spec ("Dependency policy"); no new
  dependencies without a documented blocker.
- `#![forbid(unsafe_code)]` in every project crate and fuzz target.
- `rustscript-core` stays free of filesystem, process, environment,
  networking, clock, thread, and randomness APIs, and compiles for
  `wasm32-unknown-unknown` (the `no_salsa_async_drops` cfg in
  `.cargo/config.toml` is version-coupled — revalidate on dependency bumps).
- Plain `cargo test` stays deterministic; randomized rustc-backed tests are
  `#[ignore]`d and run explicitly in CI.
- No `unwrap`/`expect`/panics on user-controlled paths in production code.

## Known danger zones

Read the surrounding comments before touching these; each encodes a
hard-won lesson:

- `crates/rustscript-core/src/frontend/lex_policy.rs` — the per-statement
  prefix-nesting bound prevents a fatal parser stack overflow that
  `catch_unwind` cannot contain. A consecutive-run bound was defeated by
  interleaved `return 1_i64 - return ...` chains; do not weaken the
  structural counting without an adversarial proof.
- `crates/rustscript-core/src/emit.rs` — two emitters exist by design
  (parsed-tree and checked-IR); the `parsed_and_checked_emitters_agree...`
  test in `tests/language.rs` is the drift guard. Lexical policy rejects
  leading-zero literals specifically so admitted literals are canonical.
- `crates/rustscript-difftest/src/lib.rs` — `PROCESS_WAIT_LOCK` and the
  serialized subprocess handling replace `wait-timeout` for a documented
  SIGCHLD reason (`docs/regression-corpus.md`); CI runs this crate with
  `--test-threads=1`.
- `crates/rustscript-wasm/host.js` — worker replacement and request
  timeouts have ordering guards (`this.worker !== worker`,
  `pending.has(id)`); preserve them.

## Verification

`scripts/verify.sh` is the gate. Individual commands, when iterating:

```text
cargo test -p rustscript-core --all-features
cargo test -p rustscript-difftest -- --test-threads=1
cargo run -p rustscript-difftest -- --seed <fresh> --cases 200
cargo clippy --workspace --all-features --all-targets -- -D warnings
wasm-pack test --node crates/rustscript-wasm
node --test crates/rustscript-wasm/host.test.mjs
cargo +nightly fuzz run parser_bytes -- -runs=100000   # longer when idle
```

Differential runs with a seed nobody has used before are cheap and find real
bugs; when in doubt, run one.
