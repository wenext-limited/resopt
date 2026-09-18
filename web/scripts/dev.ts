import page from "../generated/index.html";
import { resolve } from "node:path";
// Compile the Worker before serving; restart after changing Worker/Rust code.
// Building inside a request can contend with the HTML HMR bundler.
const root = resolve(import.meta.dir, "..");
const worker = await Bun.build({
  entrypoints: [resolve(root, "src/worker.ts")],
  target: "browser",
});
if (!worker.success)
  throw new AggregateError(worker.logs, "Worker build failed");
const workerBytes = await worker.outputs[0].arrayBuffer();
const server = Bun.serve({
  hostname: "127.0.0.1",
  port: Number(process.env.PORT || 8432),
  development: { hmr: true, console: false },
  routes: {
    "/": page,
    "/index.html": page,
    "/engine.wasm": () =>
      new Response(
        Bun.file(resolve(root, "generated/engine/resopt_wasm_bg.wasm")),
        {
          headers: {
            "Content-Type": "application/wasm",
            "Cache-Control": "no-store",
          },
        },
      ),
    "/worker.js": () =>
      new Response(workerBytes, {
        headers: {
          "Content-Type": "application/javascript",
          "Cache-Control": "no-store",
        },
      }),
  },
  fetch() {
    return new Response("Not found", { status: 404 });
  },
});
console.log(`resopt Web development: ${server.url}`);
