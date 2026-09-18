import init, { optimize_png } from "../generated/engine/resopt_wasm.js";
import type { Request, Reply } from "./types";
const ready = init({
  module_or_path: new URL("./engine.wasm", import.meta.url),
});
const worker = self as unknown as {
  onmessage: ((event: MessageEvent<Request>) => void) | null;
  postMessage: (message: Reply, transfer?: Transferable[]) => void;
};
worker.onmessage = async ({ data }) => {
  try {
    await ready;
    if (data.kind === "init") {
      worker.postMessage({ id: data.id, ok: true });
      return;
    }
    const result = optimize_png(
      new Uint8Array(data.bytes),
      data.effort,
      data.reductions,
    );
    try {
      const summary = JSON.parse(result.summary);
      const bytes = result.take_bytes().slice().buffer as ArrayBuffer;
      worker.postMessage({ id: data.id, ok: true, summary, bytes }, [bytes]);
    } finally {
      result.free();
    }
  } catch (error) {
    worker.postMessage({
      id: data.id,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
};
