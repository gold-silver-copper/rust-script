import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import test from "node:test";
import { Worker } from "node:worker_threads";

const generatedGlue = new URL("./pkg/rustscript_wasm.js", import.meta.url);

// Skip loudly rather than silently: the runner prints this message so a
// missing pkg/ directory is visible in the test summary.
const skipReason = existsSync(generatedGlue)
  ? false
  : "generated pkg/rustscript_wasm.js is missing; run `npm run build` (wasm-pack build) in crates/rustscript-wasm first";

test(
  "committed worker executes the generated wasm package",
  { skip: skipReason },
  async () => {
    const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    try {
      const result = await request(worker, {
        id: 1,
        operation: "run",
        source: 'fn main() { println!("{}", 42_i64); }',
        options: {},
      });
      assert.deepEqual(result, {
        ok: true,
        output: [52, 50, 10],
        steps: result.steps,
      });
      assert.equal(typeof result.steps, "number");

      const invalid = await request(worker, {
        id: 2,
        operation: "check",
        source: "fn main() { missing; }",
        options: {},
      });
      assert.equal(invalid.ok, false);
      assert.equal(invalid.error.phase, "type");

      const unknown = await request(worker, {
        id: 3,
        operation: "not-an-export",
        source: "",
        options: {},
      });
      assert.deepEqual(unknown, {
        ok: false,
        error: { phase: "frontend", message: "unknown operation" },
      });
    } finally {
      await worker.terminate();
    }
  },
);

function request(worker, message) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("worker request timed out")), 5000);
    worker.once("message", ({ result }) => {
      clearTimeout(timeout);
      resolve(result);
    });
    worker.once("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    worker.postMessage(message);
  });
}
