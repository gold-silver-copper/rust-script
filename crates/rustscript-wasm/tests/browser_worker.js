import { RustscriptWorkerHost } from "/host.js";

export async function browserWorkerRecovery() {
  const workers = [];
  const objectUrls = [];
  const sources = [
    `self.onmessage = () => {
      throw new Error("intentional rustscript worker abort");
    };`,
    `self.onmessage = ({ data }) => {
      self.postMessage({ id: data.id, result: { ok: true } });
    };`,
  ];
  const host = new RustscriptWorkerHost("unused-worker-url", () => {
    const source = sources[workers.length];
    if (source === undefined) throw new Error("unexpected worker generation");
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    objectUrls.push(url);
    const worker = new TrackedWorker(new Worker(url, { type: "module" }));
    workers.push(worker);
    return worker;
  });

  try {
    const failed = await withTimeout(
      host.request("check", "fn main() {}"),
      "worker abort",
    );
    if (failed.ok || failed.error?.message !== "frontend-aborted") {
      throw new Error(`unexpected failure response: ${JSON.stringify(failed)}`);
    }
    if (!workers[0].terminated) {
      throw new Error("failed worker was not terminated");
    }

    const recovered = await withTimeout(host.request("check", "fn main() {}"), "replacement");
    if (!recovered.ok) {
      throw new Error(`replacement worker did not answer: ${JSON.stringify(recovered)}`);
    }
    return workers.length;
  } finally {
    host.close();
    for (const url of objectUrls) URL.revokeObjectURL(url);
  }
}

class TrackedWorker {
  constructor(worker) {
    this.worker = worker;
    this.terminated = false;
  }

  set onmessage(handler) {
    this.worker.onmessage = handler;
  }

  set onerror(handler) {
    this.worker.onerror = handler;
  }

  set onmessageerror(handler) {
    this.worker.onmessageerror = handler;
  }

  postMessage(message) {
    this.worker.postMessage(message);
  }

  terminate() {
    this.terminated = true;
    this.worker.terminate();
  }
}

function withTimeout(promise, stage) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(
      () => reject(new Error(`worker recovery timed out during ${stage}`)),
      10_000,
    );
    promise.then(
      (value) => {
        clearTimeout(timeout);
        resolve(value);
      },
      (error) => {
        clearTimeout(timeout);
        reject(error);
      },
    );
  });
}
