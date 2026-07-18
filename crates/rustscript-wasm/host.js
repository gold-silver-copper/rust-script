/** Run untrusted rustscript requests behind a replaceable Web Worker. */
export class RustscriptWorkerHost {
  constructor(workerUrl, workerFactory = (url) => new Worker(url, { type: "module" })) {
    this.workerUrl = workerUrl;
    this.workerFactory = workerFactory;
    this.nextId = 1;
    this.pending = new Map();
    this.closed = false;
    this.replaceWorker();
  }

  request(operation, source, options = {}) {
    if (this.closed) {
      return Promise.resolve({
        ok: false,
        error: { phase: "parse", message: "frontend-aborted" },
      });
    }
    const id = this.nextId++;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.worker.postMessage({ id, operation, source, options });
    });
  }

  close() {
    this.closed = true;
    this.failPending();
    return this.worker.terminate();
  }

  replaceWorker() {
    if (this.closed) return;
    if (this.worker) this.worker.terminate();
    const worker = this.workerFactory(this.workerUrl);
    this.worker = worker;
    worker.onmessage = ({ data }) => {
      if (this.worker !== worker) return;
      const resolve = this.pending.get(data.id);
      if (resolve) {
        this.pending.delete(data.id);
        resolve(data.result);
      }
    };
    worker.onerror = () => {
      if (this.worker !== worker) return;
      this.failPending();
      this.replaceWorker();
    };
    worker.onmessageerror = () => {
      if (this.worker !== worker) return;
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
