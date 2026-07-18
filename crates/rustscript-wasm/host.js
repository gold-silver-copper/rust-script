/** Run untrusted rustscript requests behind a replaceable Web Worker. */
export class RustscriptWorkerHost {
  constructor(workerUrl) {
    this.workerUrl = workerUrl;
    this.nextId = 1;
    this.pending = new Map();
    this.replaceWorker();
  }

  request(operation, source, options = {}) {
    const id = this.nextId++;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.worker.postMessage({ id, operation, source, options });
    });
  }

  close() {
    this.worker.terminate();
    this.failPending();
  }

  replaceWorker() {
    if (this.worker) this.worker.terminate();
    this.worker = new Worker(this.workerUrl, { type: "module" });
    this.worker.onmessage = ({ data }) => {
      const resolve = this.pending.get(data.id);
      if (resolve) {
        this.pending.delete(data.id);
        resolve(data.result);
      }
    };
    this.worker.onerror = () => {
      this.failPending();
      this.replaceWorker();
    };
    this.worker.onmessageerror = () => {
      this.failPending();
      this.replaceWorker();
    };
  }

  failPending() {
    const failure = {
      ok: false,
      error: { phase: "parse", message: "frontend-aborted" },
    };
    for (const resolve of this.pending.values()) resolve(failure);
    this.pending.clear();
  }
}
