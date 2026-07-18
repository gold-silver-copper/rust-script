/** Run untrusted rustscript requests behind a replaceable Web Worker. */
export class RustscriptWorkerHost {
  constructor(
    workerUrl,
    workerFactory = (url) => new Worker(url, { type: "module" }),
    settings = {},
  ) {
    this.workerUrl = workerUrl;
    this.workerFactory = workerFactory;
    this.maxConsecutiveFailures = settings.maxConsecutiveFailures ?? 3;
    this.consecutiveFailures = 0;
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
      try {
        this.worker.postMessage({ id, operation, source, options });
      } catch {
        this.pending.delete(id);
        resolve(frontendFailure("frontend-postmessage-failed"));
      }
    });
  }

  close() {
    this.closed = true;
    this.failPending();
    return this.worker?.terminate();
  }

  replaceWorker() {
    if (this.closed) return;
    if (this.worker) this.worker.terminate();
    let worker;
    try {
      worker = this.workerFactory(this.workerUrl);
    } catch {
      this.closed = true;
      this.failPending();
      return;
    }
    this.worker = worker;
    worker.onmessage = ({ data }) => {
      if (this.worker !== worker) return;
      this.consecutiveFailures = 0;
      const resolve = this.pending.get(data.id);
      if (resolve) {
        this.pending.delete(data.id);
        resolve(data.result);
      }
    };
    worker.onerror = () => {
      if (this.worker !== worker) return;
      this.handleWorkerFailure(worker);
    };
    worker.onmessageerror = () => {
      if (this.worker !== worker) return;
      this.handleWorkerFailure(worker);
    };
  }

  handleWorkerFailure(worker) {
    this.failPending();
    this.consecutiveFailures += 1;
    if (this.consecutiveFailures > this.maxConsecutiveFailures) {
      this.closed = true;
      worker.terminate();
      return;
    }
    this.replaceWorker();
  }

  failPending() {
    const failure = frontendFailure("frontend-aborted");
    for (const resolve of this.pending.values()) resolve(failure);
    this.pending.clear();
  }
}

function frontendFailure(message) {
  return {
    ok: false,
    error: { phase: "parse", message },
  };
}
