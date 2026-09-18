import { expect, test } from "bun:test";
import { inspectPng, exportNames, size } from "../src/helpers";
import { png, crc32 } from "./fixtures";
test("archive names cannot escape and duplicate names do not overwrite", () => {
  const names = exportNames([
    "../image.png",
    "image.png",
    "image.png",
    "IMAGE.PNG",
    "report.json",
  ]);
  expect(
    names.every(
      (n) => !n.includes("/") && !n.includes("\\") && !n.startsWith("."),
    ),
  ).toBe(true);
  expect(new Set(names.map((n) => n.toLowerCase())).size).toBe(names.length);
  expect(names).not.toContain("report.json");
});
test("oversized and animated inputs are rejected before browser preview decoding", () => {
  expect(inspectPng(png(4, 3))).toEqual({ width: 4, height: 3 });
  expect(() => inspectPng(png(4, 3, false, true))).toThrow("APNG");
  const tooBig = png();
  const view = new DataView(tooBig.buffer);
  view.setUint32(16, 3000);
  view.setUint32(20, 3000);
  view.setUint32(29, crc32(tooBig.subarray(12, 29)));
  expect(() => inspectPng(tooBig)).toThrow("像素");
  expect(() => inspectPng(new Uint8Array(10))).toThrow("PNG");
  expect(() => inspectPng(png().subarray(0, 40))).toThrow("完整");
});
test("sizes use binary display units", () => {
  expect(size(0)).toBe("0 B");
  expect(size(1024)).toBe("1.0 KiB");
  expect(size(1024 * 1024)).toBe("1.0 MiB");
});
