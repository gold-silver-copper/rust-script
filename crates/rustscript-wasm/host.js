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
    // A silently reclaimed worker (for example browser OOM kill) fires no
    // error event, so every request also carries a timeout that routes
    // through the ordinary worker-failure path. 0 or null disables it.
    this.requestTimeoutMillis = settings.requestTimeoutMillis ?? 10_000;
    this.consecutiveFailures = 0;
    this.nextId = 1;
    this.pending = new Map();
    this.closed = false;
    this.replaceWorker();
  }

  request(operation, source, options = {}) {
    if (this.closed) {
      return Promise.resolve(frontendFailure("frontend-aborted"));
    }
    const id = this.nextId++;
    return new Promise((resolve) => {
      const entry = { resolve, timer: null };
      this.pending.set(id, entry);
      try {
        this.worker.postMessage({ id, operation, source, options });
      } catch {
        this.pending.delete(id);
        resolve(frontendFailure("frontend-postmessage-failed"));
        return;
      }
      if (this.requestTimeoutMillis) {
        const worker = this.worker;
        entry.timer = setTimeout(() => {
          if (!this.pending.has(id) || this.worker !== worker) return;
          this.handleWorkerFailure(worker);
        }, this.requestTimeoutMillis);
        // Do not keep the Node event loop alive for a pending timeout; a
        // browser setTimeout returns a number and the call is skipped.
        entry.timer.unref?.();
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
      const entry = this.pending.get(data.id);
      if (entry) {
        this.pending.delete(data.id);
        if (entry.timer !== null) clearTimeout(entry.timer);
        entry.resolve(data.result);
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
    for (const entry of this.pending.values()) {
      if (entry.timer !== null) clearTimeout(entry.timer);
      entry.resolve(failure);
    }
    this.pending.clear();
  }
}

// Infrastructure failures synthesized in JavaScript use their own phase so
// they are never mistaken for real parse diagnostics from the engine.
function frontendFailure(message) {
  return {
    ok: false,
    error: { phase: "frontend", message },
  };
}
