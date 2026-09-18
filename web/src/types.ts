export interface Summary {
  width: number;
  height: number;
  original_bytes: number;
  optimized_bytes: number;
  saved_bytes: number;
  transparent_pixels: number;
  pixel_equivalent: boolean;
  ssimulacra2: number;
  reductions: boolean;
}
export type Request =
  | { id: number; kind: "init" }
  | {
      id: number;
      kind: "optimize";
      bytes: ArrayBuffer;
      effort: number;
      reductions: boolean;
    };
export type Reply =
  | { id: number; ok: false; error: string }
  | { id: number; ok: true; summary?: Summary; bytes?: ArrayBuffer };
