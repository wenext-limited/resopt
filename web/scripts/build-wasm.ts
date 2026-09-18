import { join, resolve, dirname } from "node:path";
import { mkdir } from "node:fs/promises";
const web = resolve(import.meta.dir, ".."),
  root = resolve(web, "..");
async function output(args: string[]) {
  const p = Bun.spawn(args, { cwd: root, stdout: "pipe", stderr: "inherit" });
  const text = await new Response(p.stdout).text();
  if ((await p.exited) !== 0) throw Error(`Failed: ${args.join(" ")}`);
  return text.trim();
}
async function run(
  args: string[],
  env: Record<string, string | undefined> = {},
) {
  const p = Bun.spawn(args, {
    cwd: root,
    env: { ...process.env, ...env },
    stdout: "inherit",
    stderr: "inherit",
  });
  if ((await p.exited) !== 0) throw Error(`Failed: ${args.join(" ")}`);
}
const bindgen = process.env.WASM_BINDGEN || "wasm-bindgen";
if ((await output([bindgen, "--version"])) !== "wasm-bindgen 0.2.128")
  throw Error(
    "Install matching bindings: cargo install wasm-bindgen-cli --version 0.2.128 --locked",
  );
const sysroot = await output(["rustc", "--print", "sysroot"]);
const version = await output(["rustc", "-vV"]);
const host = /^host: (.+)$/m.exec(version)?.[1];
if (!host) throw Error("Cannot determine Rust host");
const ar =
  process.env.AR_wasm32_unknown_unknown ||
  join(
    sysroot,
    "lib/rustlib",
    host,
    "bin",
    process.platform === "win32" ? "llvm-ar.exe" : "llvm-ar",
  );
if (!(await Bun.file(ar).exists()))
  throw Error(
    "Install LLVM archive tools: rustup component add llvm-tools-preview",
  );
const target = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : join(root, "target");
await run(
  [
    "cargo",
    "build",
    "--locked",
    "-p",
    "resopt-wasm",
    "--lib",
    "--release",
    "--target",
    "wasm32-unknown-unknown",
  ],
  { AR_wasm32_unknown_unknown: ar, CARGO_TARGET_DIR: target },
);
const generated = join(web, "generated");
await mkdir(join(generated, "engine"), { recursive: true });
await run([
  bindgen,
  join(target, "wasm32-unknown-unknown/release/resopt_wasm.wasm"),
  "--target",
  "web",
  "--out-dir",
  join(generated, "engine"),
  "--out-name",
  "resopt_wasm",
]);
const html = await output([
  "cargo",
  "run",
  "--quiet",
  "--locked",
  "-p",
  "resopt-wasm",
  "--example",
  "web_shell",
]);
await Bun.write(join(generated, "index.html"), html);
console.log(
  `Generated browser engine and Maud HTML in ${dirname(join(generated, "index.html"))}`,
);
