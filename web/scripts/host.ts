import { resolve, join } from "node:path";
import { createNativeApi, MAX_UPLOAD } from "./native-api";
import { localOrigin } from "./host-policy";
if (process.platform !== "darwin")
  throw Error("Native image hosting requires macOS");
const hostname = process.env.HOST || "127.0.0.1";
const port = Number(process.env.PORT || 8432);
const origin = localOrigin(hostname, port, process.env.PUBLIC_ORIGIN);
const root = resolve(
  process.env.SITE_DIR || resolve(import.meta.dir, "../dist"),
);
const binary = resolve(
  process.env.RESOPT_BIN ||
    resolve(import.meta.dir, "../../target/release/resopt"),
);
if (!(await Bun.file(binary).exists()))
  throw Error("Set RESOPT_BIN to the current resopt executable");
const paths = [
  ...new Bun.Glob("**/*").scanSync({ cwd: root, onlyFiles: true }),
];
const files = new Map(paths.map((p) => ["/" + p, Bun.file(join(root, p))]));
if (!files.has("/index.html") || !files.has("/native.html"))
  throw Error("Build web assets first");
files.set("/", files.get("/index.html")!);
const api = createNativeApi(binary, origin);
const server = Bun.serve({
  hostname,
  port,
  maxRequestBodySize: MAX_UPLOAD,
  idleTimeout: 120,
  async fetch(request) {
    const url = new URL(request.url);
    if (url.origin !== origin)
      return new Response("Invalid host", { status: 403 });
    let response: Response;
    if (url.pathname === "/api/convert") response = await api(request);
    else if (!["GET", "HEAD"].includes(request.method))
      response = new Response("Method not allowed", { status: 405 });
    else if (url.pathname === "/api/capabilities")
      response = Response.json({
        native: true,
        formats: ["jpeg", "heic"],
        maxBytes: MAX_UPLOAD,
        maxPixels: 4 * 1024 * 1024,
      });
    else {
      const file = files.get(url.pathname);
      response = file
        ? new Response(request.method === "HEAD" ? null : file, {
            headers: { "Content-Type": file.type },
          })
        : new Response("Not found", { status: 404 });
    }
    response.headers.set("Cache-Control", "no-store");
    response.headers.set("X-Content-Type-Options", "nosniff");
    response.headers.set("Referrer-Policy", "same-origin");
    response.headers.set(
      "Content-Security-Policy",
      "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' blob:; worker-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
    );
    return response;
  },
});
console.log(`resopt macOS Web: ${server.url}`);
