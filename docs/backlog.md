# Backlog

The durable work queue. Agents: pick from the top, finish, verify, move the
item to the log below with the commit hash, and append anything new you
discover. Priorities follow the ranking in [`AGENTS.md`](../AGENTS.md).
Do not delete this file's structure.

## P2 — verification depth

- [ ] Run a long `parser_bytes` fuzz campaign (hours, not `-runs=100`);
      commit interesting corpus entries under `fuzz/corpus/parser_bytes/`
      and minimize any finding into the regression corpus. This is the
      highest-value unexecuted validation in the project.
- [ ] Run a long `ast_roundtrip` campaign the same way.
- [ ] Differential volume: run `rustscript-difftest` with several fresh
      seeds at `--cases 5000+`; record seeds tried (and results) here so
      seeds are not repeated: tried so far — seed 1 (×1000, ×200, ×100),
      seed 7 (×100), seed 12345 (spot).
- [ ] Exact-boundary limit tests: accept at exactly `max_tokens` /
      delimiter depth 256 / syntax depth 256, reject at limit+1 (only the
      prefix-nesting bound has these today).
- [ ] Property test that `format_program` output always reparses+rechecks
      to structurally equal IR for arbitrary *admitted* (not generated)
      sources harvested from the fuzz corpus.

## P3 — robustness & tooling polish

- [ ] Chrome/chromedriver run in the CI wasm job alongside Firefox
      (worker `error`-event semantics are the engine-variant risk).
- [ ] `--artifacts PATH` flag for `rustscript-difftest` (root is currently
      hardcoded cwd-relative `artifacts/differential`).
- [ ] Oracle reader-drain timeout currently surfaces as a hard
      `NativeFailure`; consider returning captured bytes with the existing
      `OutputCaptureTruncated` classification instead.
- [ ] Windows: `terminate_process_tree` kills only the direct child
      (documented). Real fix needs Job Objects; needs a Windows CI runner
      to validate — otherwise leave documented.
- [ ] Note the dropped spec-listed `serde` dependency of
      `rustscript-difftest` in `docs/tooling-reuse.md` (removed as unused;
      unlike `wait-timeout`, the deviation is currently undocumented).

## Needs human decision (do not implement unilaterally)

- Multi-error reporting at lex/parse: the spec's
  `parse -> Result<_, Diagnostic>` signature forces first-error-only.
  Aggregating requires a public API change.
- Any language-subset extension (more types, `for`, strings, etc.) —
  a spec change, not an improvement.
- Batched rustc invocations in the differential runner (spec allows it
  only after correctness is established; decide if the compile-time win
  matters).

## Done log

- 2026-07-18 `651f64b`..`da09f51` — spec-conformance review round: prefix
  stack-overflow class fixed twice (homogeneous runs, then interleaved
  chains defeating the run counter), public API aligned to spec surface,
  partial-stdout runtime failures, multi-diagnostic `check`, admission-phase
  reserved-name/comparison-chain checks, emitter drift guard (found a real
  empty-block bug), generator strengthening (4096-decision streams,
  ±1000 literals, computed operands, bounded `break`), worker-host request
  timeout, leading-zero literal rejection, CI/docs gaps closed.
