/** Native image access is restricted to the same device, never a LAN host. */
export function localOrigin(host: string, port: number, publicOrigin?: string) {
  const origin = `http://127.0.0.1:${port}`;
  if (
    host !== "127.0.0.1" ||
    (publicOrigin !== undefined && publicOrigin !== origin)
  )
    throw Error(
      "Native image processing is local-only; use resopt web /path/to/project. Remote hosts may only serve the static WASM site.",
    );
  return origin;
}
