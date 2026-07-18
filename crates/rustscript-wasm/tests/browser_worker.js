import { RustscriptWorkerHost } from "/host.js";

export function browserWorkerRecovery() {
  const workers = [];
  const host = new RustscriptWorkerHost("unused-worker-url", () => {
    const worker = {
      terminated: false,
      terminate() {
        this.terminated = true;
      },
      postMessage() {
        throw new Error("the Rust test drives the browser failure path directly");
      },
    };
    workers.push(worker);
    return worker;
  });

  try {
    let failed;
    host.pending.set(1, (result) => {
      failed = result;
    });
    host.handleWorkerFailure(workers[0]);
    if (failed.ok || failed.error?.message !== "frontend-aborted") {
      throw new Error(`unexpected failure response: ${JSON.stringify(failed)}`);
    }
    if (!workers[0].terminated) {
      throw new Error("failed worker was not terminated");
    }

    let recovered;
    host.pending.set(2, (result) => {
      recovered = result;
    });
    workers[1].onmessage({ data: { id: 2, result: { ok: true } } });
    if (!recovered.ok) {
      throw new Error(`replacement worker did not answer: ${JSON.stringify(recovered)}`);
    }
    return workers.length;
  } finally {
    host.close();
  }
}
