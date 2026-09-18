import { resolve, join } from "node:path";
import { mkdir, rm } from "node:fs/promises";
const web = resolve(import.meta.dir, ".."),
  outdir = join(web, "dist");
await rm(outdir, { recursive: true, force: true });
await mkdir(outdir, { recursive: true });
for (const [entry, naming] of [
  [join(web, "generated/index.html"), "[name]-[hash].[ext]"],
  [join(web, "src/worker.ts"), "[name].[ext]"],
] as const) {
  const result = await Bun.build({
    entrypoints: [entry],
    outdir,
    target: "browser",
    minify: true,
    naming: { entry: naming, asset: "[name]-[hash].[ext]" },
  });
  if (!result.success)
    throw new AggregateError(result.logs, "Browser build failed");
  const html = result.outputs.find((o) => o.path.endsWith(".html"));
  if (html && !html.path.endsWith("/index.html")) {
    await Bun.write(join(outdir, "index.html"), html);
    await rm(html.path);
  }
}
// Keep engine and Worker as same-origin static files. Nothing handles uploads.
await Bun.write(
  join(outdir, "engine.wasm"),
  Bun.file(join(web, "generated/engine/resopt_wasm_bg.wasm")),
);
console.log("Static browser app: " + outdir);
