#!/usr/bin/env bash
# The verification gate. Every agent session ends with this green.
# Fast native gate by default; FULL=1 adds wasm, npm, browser, and fuzz
# builds (requires wasm-pack, node, Firefox, and a nightly toolchain).
set -euo pipefail
cd "$(dirname "$0")/.."

run() { echo "==> $*"; "$@"; }

run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets -- -D warnings
run cargo clippy --workspace --all-features --all-targets -- -D warnings
RUSTDOCFLAGS='-D missing-docs' run cargo doc --workspace --no-deps --quiet
run cargo test --workspace --exclude rustscript-difftest
run cargo test -p rustscript-core --all-features
run cargo test -p rustscript-difftest -- --test-threads=1
run cargo test -p rustscript-difftest -- --test-threads=1 --ignored
run cargo run -q -p rustscript-difftest -- --seed 1 --cases 25
run cargo check -p rustscript-wasm --target wasm32-unknown-unknown

if [[ "${FULL:-0}" == "1" ]]; then
  run wasm-pack test --node crates/rustscript-wasm
  run wasm-pack test --headless --firefox crates/rustscript-wasm --test browser
  run npm --prefix crates/rustscript-wasm run build
  run npm --prefix crates/rustscript-wasm test
  run npm --prefix crates/rustscript-wasm run pack:check
  host="$(rustc +nightly -vV | sed -n 's/^host: //p')"
  (cd fuzz && run cargo +nightly fuzz build parser_bytes --target "$host")
  (cd fuzz && run cargo +nightly fuzz build ast_roundtrip --target "$host")
fi

echo "verify: all green"
