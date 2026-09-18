import { resolve, join } from "node:path";
const root = resolve(
  process.env.SITE_DIR || resolve(import.meta.dir, "../dist"),
);
const paths = [
  ...new Bun.Glob("**/*").scanSync({ cwd: root, onlyFiles: true }),
];
const files = new Map(
  paths.map((path) => ["/" + path, Bun.file(join(root, path))]),
);
const index =
  paths.find((path) => path === "index.html") ||
  paths.find((path) => path.startsWith("index-") && path.endsWith(".html"));
if (!index) throw Error("Run bun run build first");
files.set("/", Bun.file(join(root, index)));
const server = Bun.serve({
  hostname: process.env.HOST || "127.0.0.1",
  maxRequestBodySize: 1024,
  port: Number(process.env.PORT || 8432),
  fetch(request) {
    if (!["GET", "HEAD"].includes(request.method))
      return new Response("Method not allowed", { status: 405 });
    const path = new URL(request.url).pathname;
    if (path === "/api/capabilities")
      return Response.json({ native: false, mode: "browser-only" });
    if (path === "/native.html")
      return new Response(null, {
        status: 302,
        headers: { Location: "/", "Cache-Control": "no-store" },
      });
    const file = files.get(path);
    if (!file) return new Response("Not found", { status: 404 });
    return new Response(request.method === "HEAD" ? null : file, {
      headers: {
        "Content-Type": file.type,
        "Cache-Control": "no-store",
        "X-Content-Type-Options": "nosniff",
        "Content-Security-Policy":
          "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' blob:; worker-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
      },
    });
  },
});
console.log(`resopt Web: ${server.url}`);
