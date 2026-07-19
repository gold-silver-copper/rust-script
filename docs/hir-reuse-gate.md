# The HIR-reuse gate: coupling subset growth to rust-analyzer semantics

This document is the decision procedure for **when rustscript should embed
rust-analyzer's HIR / type-inference stack instead of hand-rolling its
checker.** It supersedes the one-time "no, it's blocked" note in
[`tooling-reuse.md`](tooling-reuse.md) with a *living, re-runnable gate* whose
outcome is coupled to how far the language subset has grown.

## The principle

The whole project rests on one invariant: every accepted program compiles with
`rustc --edition=2024` and produces byte-identical stdout. The checker exists
only to keep the accepted set inside that invariant. Two forces pull on the
checker as the language grows:

- **A hand-rolled checker is right while the type system is small.** For the V0
  three-type profile (`i64`, `bool`, `()`, value semantics, no heap, no
  borrow), a lexical-scope checker is a few hundred lines, has no runtime
  dependencies, is trivially WASM-safe and deterministic, and gives exact
  diagnostics. Embedding a 200+‑crate compiler to type three scalars would be
  absurd.
- **A hand-rolled checker becomes a second compiler as the type system grows.**
  The moment the subset wants **references/borrows, generics, traits, or
  inference** — anything requiring a real type solver — reimplementing it beside
  rust-analyzer is exactly the "second frontend" the project forbids elsewhere.
  There, embedding rust-analyzer's inference is the *reuse-first* choice.

So subset expansion and the embed/hand-roll decision are the **same decision**.
This gate binds them: a feature below the threshold ships on the hand-rolled
checker; a feature at or above it may not be implemented until the gate is
re-run and passes.

## The threshold

| Tier | Features | Checker strategy |
|------|----------|------------------|
| **Value-semantics envelope** (V0, V1, …) | more scalar types (`i32`, `u64`, …, each with exact overflow semantics), `for` over ranges, `match` on scalars, compound assignment, value tuples/arrays with bounds, labelled loops | **Hand-rolled.** Each feature is "more of the same": a known rustc-equivalent, differentially provable, no new type-system machinery. Do **not** re-run the gate. |
| **Semantic-domain frontier** | references / borrows, generics, traits / bounds, closures capturing environment, inference variables, associated types | **Gate required.** Do not implement until the gate below passes; if it passes, embed rust-analyzer inference and keep only rustscript's admission-whitelist and execution checks on top. |
| **Out of profile entirely** | heap (`String`, `Vec`, `Box`), floats, `unsafe`, macros | Separate major-version decision with its own differential strategy; the gate is necessary but not sufficient (heap/float also need `Display`/IEEE-754 stdout-equivalence work). |

The load-bearing rule: **you may add value-semantics features freely, but the
first borrow/generics/trait feature triggers a mandatory gate re-run, not a pile
of new hand-written type rules.**

## The gate (reproducible experiment)

Re-run on every rust-analyzer pin bump and before implementing any
semantic-domain-frontier feature. Pass requires **all** of:

1. **API fit** — single in-memory Edition 2024 file → resolved local/function
   types through a public API, no test-only crates on the production path.
2. **WASM compile** — the inference path compiles for
   `wasm32-unknown-unknown`.
3. **Browser execution** — the same path actually runs in a headless browser
   (not just links), with no background thread / unbounded work invoked on the
   single-file inference path.
4. **Determinism & bounded resources** — repeatable results; the resource
   limits (fuel-equivalent, timeouts) still bound adversarial input.
5. **Diagnostic sufficiency** — enough resolution/type facts to enforce every
   subset rule and emit useful spans, without a second checker beside it.
6. **Cost** — acceptable release size / build time for a browser scripting
   engine.

## Current re-run: 2026-07-19 (pin `=0.0.342`, Rust 1.95)

This re-run went materially further than the 2026-07-18 gate and **overturns its
central finding.** Ephemeral crate (not added to the workspace — the repo must
not gain these deps until the gate fully passes):

```toml
# hir-spike/Cargo.toml — direct deps (all =0.0.342 except salsa/triomphe)
ra_ap_ide, ra_ap_ide_db, ra_ap_hir, ra_ap_hir_ty, ra_ap_syntax,
ra_ap_base_db, ra_ap_vfs, ra_ap_paths, ra_ap_test_fixture,
salsa = "=0.27.2", triomphe = "0.1"
```

```rust
// The path the 2026-07-18 spike declared blocked, now working:
let (db, file_id) = RootDatabase::with_single_file(source);   // concrete DB, one file
let krate  = Crate::all(&db).into_iter().next().unwrap();
let target = DisplayTarget::from_crate(&db, krate.into());
let hir_db: &dyn HirDatabase = &db;
ra_ap_hir_ty::attach_db(hir_db, || {                          // REQUIRED: see below
    let sema = Semantics::new(&db);
    let file = sema.parse(file_id);
    // for each `let`: sema.type_of_expr(&initializer) -> Type
});
```

Commands: `cargo +1.95.0 run` (native), then
`cargo +1.95.0 check --target wasm32-unknown-unknown` with
`.cargo/config.toml` setting `--cfg no_salsa_async_drops`.

### Results against the six criteria

| # | Criterion | 2026-07-18 | 2026-07-19 re-run |
|---|-----------|------------|-------------------|
| 1 | API fit | **Blocked** ("no small public constructor") | **Passes for inference.** `RootDatabase::with_single_file` + `Semantics::type_of_expr` correctly infers `value : i64` (through a call `add(20_i64, 22_i64)`), `flag : bool` (through `<`), `unit : ()`, **deterministically across two passes**. Caveat: `with_single_file` is from the **test** crate `ra_ap_test_fixture`; the production single-file entry is `ra_ap_ide::Analysis::from_single_file(text, proc_macro_cwd)` — proven to exist and compile, but wiring `Semantics` onto its database is not yet demonstrated (open item). |
| 2 | WASM compile | Link-only deps check (14 s) | **Passes.** The full inference path — `ide_db`, `hir_ty`, the next trait solver, `attach_db`, `type_of_expr` — checks for `wasm32-unknown-unknown` in ~19 s. |
| 3 | Browser execution | Not attempted | **Still unproven.** Compiles for wasm but not yet run in a headless browser. This is now the decisive open criterion. |
| 4 | Determinism / bounds | Not reached | Native determinism observed (two passes identical). Resource-bounding under adversarial input **not** yet integrated or measured. |
| 5 | Diagnostic sufficiency | Not reached | Types resolve; whether HIR diagnostics cover every subset rule with good spans is **unassessed**. |
| 6 | Cost | 137 pkgs (semantic-only tree) | wasm normal-dep graph for the full inference path is **~225 packages**; bundle-size impact for the browser engine **unmeasured**. |

### The new hard constraint the re-run exposed

The next trait solver requires a **thread-local, raw-pointer database
attachment** (`ra_ap_hir_ty::attach_db` → `next_solver::interner::tls_db`, a
non-reentrant `thread_local! { Cell<Option<NonNull<dyn HirDatabase>>> }` that
**panics** on nesting or on a changed db). Every inference call must be wrapped
in `attach_db`. On `wasm32-unknown-unknown` (single-threaded) a thread-local is
just a global, so this is workable, but it is a reentrancy/aliasing hazard that
the rustscript adapter would have to encapsulate carefully and never expose.

## Decision (as of 2026-07-19)

**Do not embed yet, but the gate is no longer closed — it is a live
trade-off.** The two blockers that made it a hard "no" (API constructor, wasm
compile) are resolved. What remains before a semantic-domain-frontier feature
could justify embedding: **prove browser execution (criterion 3), integrate
resource-bounding (4), and measure bundle cost (6)**, plus find a production
(non-test-fixture) database path.

Concretely:

- **Stay hand-rolled for the value-semantics envelope (V0/V1).** Embedding 225
  crates + a thread-local-attach hazard to type scalars is not the trade.
- **When the first borrow/generics/trait feature is proposed, finish the
  gate** (items 3, 4, 6 + production path). If it passes, embed rust-analyzer
  inference behind a private adapter and keep only rustscript's admission
  whitelist and execution checks; do **not** grow a hand-written borrow
  checker or trait solver.
- **Re-run this gate on every `ra_ap_*` pin bump** and update the table above.

## Reproduction

The spike is intentionally kept out of the workspace. To reproduce, recreate
the ephemeral crate from the dependency list and code above, then run the two
commands. The exact resolved semantic-stack graph is in
[`hir-spike-tree.md`](hir-spike-tree.md).
