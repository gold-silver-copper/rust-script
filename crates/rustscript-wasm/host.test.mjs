import assert from "node:assert/strict";
import test from "node:test";
import { Worker } from "node:worker_threads";

import { RustscriptWorkerHost } from "./host.js";

class FakeWorker {
  constructor() {
    this.messages = [];
    this.terminated = false;
  }

  postMessage(message) {
    this.messages.push(message);
  }

  terminate() {
    this.terminated = true;
  }

  succeed(result) {
    const message = this.messages.shift();
    this.onmessage({ data: { id: message.id, result } });
  }

  abort() {
    this.onerror(new Error("intentional worker termination"));
  }
}

class ThrowingWorker extends FakeWorker {
  postMessage() {
    throw new Error("cannot clone request");
  }
}

test("reports frontend abort and replaces the failed worker", async () => {
  const workers = [];
  const host = new RustscriptWorkerHost("worker.js", () => {
    const worker = new FakeWorker();
    workers.push(worker);
    return worker;
  });

  const failedRequest = host.request("check", "fn main() {}");
  workers[0].abort();
  assert.deepEqual(await failedRequest, {
    ok: false,
    error: { phase: "frontend", message: "frontend-aborted" },
  });
  assert.equal(workers[0].terminated, true);
  assert.equal(workers.length, 2);

  const successfulRequest = host.request("check", "fn main() {}");
  workers[1].succeed({ ok: true });
  assert.deepEqual(await successfulRequest, { ok: true });
  host.close();
});

test("does not leak pending calls when postMessage throws synchronously", async () => {
  const host = new RustscriptWorkerHost("worker.js", () => new ThrowingWorker());
  const result = await host.request("check", () => {});
  assert.deepEqual(result, {
    ok: false,
    error: { phase: "frontend", message: "frontend-postmessage-failed" },
  });
  assert.equal(host.pending.size, 0);
  host.close();
});

test("bounds consecutive deterministic worker replacement failures", async () => {
  const workers = [];
  const host = new RustscriptWorkerHost(
    "worker.js",
    () => {
      const worker = new FakeWorker();
      workers.push(worker);
      return worker;
    },
    { maxConsecutiveFailures: 1 },
  );

  const first = host.request("check", "fn main() {}");
  workers[0].abort();
  assert.deepEqual(await first, {
    ok: false,
    error: { phase: "frontend", message: "frontend-aborted" },
  });
  assert.equal(workers.length, 2);

  const second = host.request("check", "fn main() {}");
  workers[1].abort();
  assert.deepEqual(await second, {
    ok: false,
    error: { phase: "frontend", message: "frontend-aborted" },
  });
  assert.equal(host.closed, true);
  assert.equal(workers.length, 2);
});

test("times out a silently killed worker and replaces it", async () => {
  const workers = [];
  const host = new RustscriptWorkerHost(
    "worker.js",
    () => {
      const worker = new FakeWorker();
      workers.push(worker);
      return worker;
    },
    { requestTimeoutMillis: 20 },
  );

  // The first worker receives the request but never answers and never fires
  // an error event, mimicking a browser reclaiming the worker process.
  const hung = await host.request("check", "fn main() {}");
  assert.deepEqual(hung, {
    ok: false,
    error: { phase: "frontend", message: "frontend-aborted" },
  });
  assert.equal(workers[0].terminated, true);
  assert.equal(workers.length, 2);
  assert.equal(host.pending.size, 0);

  const recovered = host.request("check", "fn main() {}");
  workers[1].succeed({ ok: true });
  assert.deepEqual(await recovered, { ok: true });
  host.close();
});

test("a timeout of zero disables the per-request deadline", async () => {
  const workers = [];
  const host = new RustscriptWorkerHost(
    "worker.js",
    () => {
      const worker = new FakeWorker();
      workers.push(worker);
      return worker;
    },
    { requestTimeoutMillis: 0 },
  );

  const request = host.request("check", "fn main() {}");
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.equal(host.pending.size, 1);
  workers[0].succeed({ ok: true });
  assert.deepEqual(await request, { ok: true });
  assert.equal(workers.length, 1);
  host.close();
});

class NodeWorkerAdapter {
  constructor(source) {
    this.worker = new Worker(source, { eval: true, type: "module" });
    this.worker.on("message", (data) => this.onmessage?.({ data }));
    this.worker.on("messageerror", (error) => this.onmessageerror?.(error));
    this.worker.on("error", (error) => this.onerror?.(error));
    this.worker.on("exit", (code) => {
      if (code !== 0) this.onerror?.(new Error(`worker exited with ${code}`));
    });
  }

  postMessage(message) {
    this.worker.postMessage(message);
  }

  terminate() {
    return this.worker.terminate();
  }
}

test("recovers after an actual worker thread terminates", async () => {
  let generation = 0;
  const host = new RustscriptWorkerHost("unused", () => {
    generation += 1;
    const source = generation === 1
      ? `import { parentPort } from "node:worker_threads";
         parentPort.on("message", () => process.exit(23));`
      : `import { parentPort } from "node:worker_threads";
         parentPort.on("message", ({ id }) => parentPort.postMessage({ id, result: { ok: true } }));`;
    return new NodeWorkerAdapter(source);
  });

  const failed = await host.request("check", "fn main() {}");
  assert.deepEqual(failed, {
    ok: false,
    error: { phase: "frontend", message: "frontend-aborted" },
  });
  assert.equal(generation, 2);

  const recovered = host.request("check", "fn main() {}");
  assert.deepEqual(await recovered, { ok: true });
  await host.close();
});
