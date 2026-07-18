import init, * as rustscript from "./pkg/rustscript_wasm.js";

await init();

self.onmessage = ({ data }) => {
  const { id, operation, source, options } = data;
  const handler = rustscript[operation];
  if (typeof handler !== "function") {
    self.postMessage({ id, result: { ok: false, error: { phase: "parse", message: "unknown operation" } } });
    return;
  }
  self.postMessage({ id, result: handler(source, options) });
};
