import init, * as rustscript from "./pkg/rustscript_wasm.js";

const browserScope = globalThis.self;
let parentPort = null;
if (!browserScope || typeof browserScope.postMessage !== "function") {
  try {
    ({ parentPort } = await import("node:worker_threads"));
  } catch {
    parentPort = null;
  }
}

if (parentPort) {
  const { readFile } = await import("node:fs/promises");
  const wasmBytes = await readFile(new URL("./pkg/rustscript_wasm_bg.wasm", import.meta.url));
  rustscript.initSync({ module: wasmBytes });
} else {
  await init();
}

const postResult = (message) => {
  if (parentPort) {
    parentPort.postMessage(message);
  } else {
    browserScope.postMessage(message);
  }
};

const handleMessage = ({ data }) => {
  const { id, operation, source, options } = data;
  const handler = rustscript[operation];
  if (typeof handler !== "function") {
    postResult({ id, result: { ok: false, error: { phase: "parse", message: "unknown operation" } } });
    return;
  }
  postResult({ id, result: handler(source, options) });
};

if (parentPort) {
  parentPort.on("message", (data) => handleMessage({ data }));
} else {
  browserScope.onmessage = handleMessage;
}
