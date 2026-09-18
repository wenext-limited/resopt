import { mkdtemp, mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";

export const MAX_UPLOAD = 16 * 1024 * 1024;
const MAX_PIXELS = 4 * 1024 * 1024;
export function parseOptions(url: URL) {
  const format = url.searchParams.get("format");
  const quality = Number(url.searchParams.get("quality"));
  if (
    !["auto", "png", "jpeg", "heic"].includes(format || "") ||
    ![75, 85, 95].includes(quality)
  )
    throw Error("请选择有效的格式和 75、85、95 质量档位");
  return { format: format!, quality };
}
export function detectInput(bytes: Uint8Array): string {
  if (bytes.length > MAX_UPLOAD || bytes.length < 12)
    throw Error("图片须小于 16 MiB");
  if ([137, 80, 78, 71, 13, 10, 26, 10].every((v, i) => bytes[i] === v))
    return "png";
  if (bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255) return "jpeg";
  const ascii = (start: number, end: number) =>
    new TextDecoder().decode(bytes.slice(start, end));
  if (
    ascii(4, 8) === "ftyp" &&
    ["heic", "heix", "hevc", "hevx", "mif1", "msf1"].includes(ascii(8, 12))
  )
    return "heic";
  throw Error("仅接受 PNG、JPEG 或 HEIC 图片");
}
export async function artifact(
  directory: string,
  path: unknown,
): Promise<string> {
  if (typeof path !== "string") throw Error("没有可下载的候选");
  const full = resolve(directory, path);
  if (!full.startsWith(resolve(directory) + sep)) throw Error("无效的候选路径");
  const file = Bun.file(full);
  if (file.size > 32 * 1024 * 1024) throw Error("候选超出大小限制");
  return Buffer.from(await file.arrayBuffer()).toString("base64");
}
export function createNativeApi(binary: string, origin: string) {
  let busy = false;
  return async (request: Request): Promise<Response> => {
    const fail = (message: string, status = 400) =>
      Response.json({ error: message }, { status });
    if (request.method !== "POST") return fail("仅支持 POST", 405);
    if (
      request.headers.get("origin") !== origin ||
      request.headers.get("x-resopt-request") !== "1"
    )
      return fail("只接受当前网页发起的请求", 403);
    if (busy) return fail("服务器正在处理另一张图片，请稍后重试", 429);
    busy = true;
    let directory: string | undefined;
    try {
      const options = parseOptions(new URL(request.url));
      if (Number(request.headers.get("content-length")) > MAX_UPLOAD)
        return fail("图片超过 16 MiB", 413);
      const bytes = new Uint8Array(await request.arrayBuffer());
      const extension = detectInput(bytes);
      directory = await mkdtemp(join(tmpdir(), "resopt-web-"));
      const input = join(directory, "input"),
        out = join(directory, "output");
      await mkdir(input, { mode: 0o700 });
      await Bun.write(join(input, `image.${extension}`), bytes);
      const proc = Bun.spawn(
        [
          binary,
          "analyze",
          input,
          "--out",
          out,
          "--qualities",
          String(options.quality),
          "--jobs",
          "1",
          "--max-pixels",
          String(MAX_PIXELS),
          "--png-level",
          "1",
          "--json",
        ],
        { stdout: "ignore", stderr: "pipe" },
      );
      let timedOut = false;
      const timer = setTimeout(() => {
        timedOut = true;
        proc.kill("SIGKILL");
      }, 90000);
      const abort = () => proc.kill("SIGKILL");
      request.signal.addEventListener("abort", abort, { once: true });
      let code: number;
      try {
        // Drain bounded CLI diagnostics; no submitted filename enters the process arguments.
        await new Response(proc.stderr).text();
        code = await proc.exited;
      } finally {
        clearTimeout(timer);
        request.signal.removeEventListener("abort", abort);
      }
      if (timedOut) return fail("处理超过 90 秒，请使用更小图片", 504);
      if (code !== 0)
        return fail("图片分析失败：请检查编码、尺寸或尝试原生 CLI", 422);
      const report = await Bun.file(join(out, "analysis.json")).json();
      const row = report.resources?.[0];
      if (!row?.image) return fail("无法解码图片，或超过 4 MP 像素上限", 422);
      if (row.image.frames !== 1) return fail("暂不支持多帧图片", 422);
      if (options.format === "jpeg" && row.image.has_transparent_pixels)
        return fail("图片含透明像素，JPEG 无法保留透明度，请选择 HEIC", 422);
      const candidates =
        row.candidates?.filter(
          (c: any) => options.format === "auto" || c.format === options.format,
        ) || [];
      const candidate =
        candidates
          .filter((c: any) => c.valid && c.artifact)
          .sort((a: any, b: any) => a.bytes - b.bytes)[0] ||
        candidates.find((c: any) => c.valid);
      if (!candidate?.valid)
        return fail("候选未通过透明度或编码校验，请换一个质量档位", 422);
      if (!candidate.artifact)
        return Response.json({
          improved: false,
          original_bytes: bytes.length,
          candidate_bytes: candidate.bytes,
          message: "该格式与质量没有减小体积，已保留原图。",
        });
      return Response.json({
        improved: true,
        format: candidate.format,
        quality: candidate.quality,
        width: row.image.width,
        height: row.image.height,
        transparent_pixels: row.image.transparent_pixels,
        original_bytes: bytes.length,
        optimized_bytes: candidate.bytes,
        saved_bytes: candidate.savings_bytes,
        difference: candidate.difference,
        bytes: await artifact(out, candidate.artifact),
        originalPreview: await artifact(out, row.original_preview),
        candidatePreview: await artifact(out, candidate.preview),
      });
    } catch {
      return fail("请求无效或图片处理失败，请检查格式和质量参数", 422);
    } finally {
      try {
        if (directory) await rm(directory, { recursive: true, force: true });
      } finally {
        busy = false;
      }
    }
  };
}
