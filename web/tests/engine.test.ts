import { beforeAll, expect, test } from "bun:test";
import init, {
  optimize_png,
  score_srgb_rgba,
} from "../generated/engine/resopt_wasm.js";
import { png } from "./fixtures";
let wasm: Awaited<ReturnType<typeof init>>;
beforeAll(async () => {
  wasm = await init({
    module_or_path: await Bun.file(
      new URL("../generated/engine/resopt_wasm_bg.wasm", import.meta.url),
    ).arrayBuffer(),
  });
});
test("real WASM optimizes opaque and transparent PNGs and verifies pixels", () => {
  for (const transparent of [false, true])
    for (const reductions of [false, true]) {
      const input = png(64, 64, transparent),
        result = optimize_png(input, 2, reductions);
      try {
        const summary = JSON.parse(result.summary),
          bytes = result.take_bytes();
        expect(summary.pixel_equivalent).toBe(true);
        expect(summary.ssimulacra2).toBeCloseTo(100, 2);
        expect(bytes.length).toBeLessThan(input.length);
        expect(summary.optimized_bytes).toBe(bytes.length);
        expect(summary.transparent_pixels > 0).toBe(transparent);
        expect(result.take_bytes().length).toBe(0);
      } finally {
        result.free();
      }
    }
});
test("WASM quality scoring notices RGB and alpha changes", () => {
  const a = new Uint8Array(64 * 64 * 4).fill(255),
    b = a.slice();
  for (let i = 0; i < b.length; i += 4) b[i] = 0;
  expect(score_srgb_rgba(64, 64, a, a)).toBeCloseTo(100, 2);
  expect(score_srgb_rgba(64, 64, a, b)).toBeLessThan(100);
  for (let i = 3; i < b.length; i += 4) b[i] = 0;
  expect(score_srgb_rgba(64, 64, a, b)).toBeLessThan(100);
  expect(() => score_srgb_rgba(64, 64, a, b.subarray(4))).toThrow(
    "length mismatch",
  );
});
test("bad inputs fail without poisoning the WASM engine", () => {
  expect(() => optimize_png(new Uint8Array([1, 2, 3]), 2, true)).toThrow();
  expect(() => optimize_png(png(), 9, true)).toThrow("effort");
  expect(() => optimize_png(png(4, 4, false, true), 2, true)).toThrow(
    "animated",
  );
  const result = optimize_png(png(), 2, true);
  result.free();
});
test("a full HD input stays within the browser pixel budget", () => {
  const result = optimize_png(png(1920, 1080), 1, true);
  try {
    expect(JSON.parse(result.summary).width).toBe(1920);
    expect(JSON.parse(result.summary).pixel_equivalent).toBe(true);
    expect(wasm.memory.buffer.byteLength).toBeLessThan(1024 * 1024 * 1024);
  } finally {
    result.free();
  }
}, 30000);
