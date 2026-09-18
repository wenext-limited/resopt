import { zlibSync } from "fflate";
export function crc32(bytes: Uint8Array) {
  let c = 0xffffffff;
  for (const b of bytes) {
    c ^= b;
    for (let i = 0; i < 8; i++) c = (c >>> 1) ^ (c & 1 ? 0xedb88320 : 0);
  }
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type: string, data: Uint8Array) {
  const out = new Uint8Array(data.length + 12),
    view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  out.set(new TextEncoder().encode(type), 4);
  out.set(data, 8);
  view.setUint32(out.length - 4, crc32(out.subarray(4, out.length - 4)));
  return out;
}
function concat(parts: Uint8Array[]) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}
export function png(
  width = 64,
  height = 64,
  transparent = false,
  animated = false,
) {
  const header = new Uint8Array(13),
    view = new DataView(header.buffer);
  view.setUint32(0, width);
  view.setUint32(4, height);
  header[8] = 8;
  header[9] = 6;
  const rows = new Uint8Array(height * (width * 4 + 1));
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++)
      rows.set(
        [x % 256, y % 256, 140, transparent && x < width / 2 ? 128 : 255],
        y * (width * 4 + 1) + 1 + x * 4,
      );
  const parts = [
    new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
  ];
  if (animated) {
    const animation = new Uint8Array(8);
    new DataView(animation.buffer).setUint32(0, 1);
    parts.push(chunk("acTL", animation));
  }
  parts.push(
    chunk("IDAT", zlibSync(rows, { level: 0 })),
    chunk("IEND", new Uint8Array()),
  );
  return concat(parts);
}
