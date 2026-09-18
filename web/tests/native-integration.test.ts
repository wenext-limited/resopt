import { test, expect } from "bun:test";
import { createNativeApi } from "../scripts/native-api";
import { png } from "./fixtures";
const binary = process.env.RESOPT_TEST_BIN;
const native = binary && process.platform === "darwin" ? test : test.skip;
native(
  "macOS API encodes JPEG/HEIC, rejects alpha loss, and keeps no-gain originals",
  async () => {
    const api = createNativeApi(binary!, "http://localhost:8434");
    const send = (body: Uint8Array, format: string) =>
      api(
        new Request(
          `http://localhost:8434/api/convert?format=${format}&quality=85`,
          {
            method: "POST",
            headers: {
              Origin: "http://localhost:8434",
              "X-Resopt-Request": "1",
            },
            body: body.slice().buffer,
          },
        ),
      );
    const jpeg = await send(png(160, 160), "jpeg");
    expect(jpeg.status).toBe(200);
    const j = await jpeg.json();
    expect(j.improved).toBe(true);
    expect(j.format).toBe("jpeg");
    expect(j.difference.ssimulacra2).toBeGreaterThan(50);
    const heic = await send(png(160, 160, true), "heic");
    expect(heic.status).toBe(200);
    const h = await heic.json();
    expect(h.improved).toBe(true);
    expect(h.transparent_pixels).toBeGreaterThan(0);
    expect(h.difference.max_alpha_error).toBeLessThanOrEqual(
      1 / 255 + 0.000001,
    );
    const rejected = await send(png(160, 160, true), "jpeg");
    expect(rejected.status).toBe(422);
    expect((await rejected.json()).error).toContain("透明");
    const again = await send(
      new Uint8Array(Buffer.from(h.bytes, "base64")),
      "heic",
    );
    expect(again.status).toBe(200);
    const a = await again.json();
    if (a.improved) expect(a.optimized_bytes).toBeLessThan(a.original_bytes);
    else expect(a.message).toContain("保留原图");
  },
  60000,
);
