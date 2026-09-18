export const MAX_FILES = 500;
export const MAX_FILE_BYTES = 16 * 1024 * 1024;
export const MAX_OUTPUT_BYTES = 128 * 1024 * 1024;
export function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB"];
  let n = bytes / 1024,
    i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toLocaleString("zh-CN", { maximumFractionDigits: 2, minimumFractionDigits: 1 })} ${units[i]}`;
}
export function safeName(name: string) {
  return (
    name.replace(/[\\/\u0000-\u001f<>:"|?*]/g, "_").replace(/^\.+/, "_") ||
    "image.png"
  );
}
export function exportNames(names: string[]) {
  const used = new Set<string>(["report.json"]);
  return names.map((name) => {
    const base = safeName(name);
    const safe = base.toLowerCase().endsWith(".png") ? base : base + ".png";
    let value = safe,
      index = 2;
    while (used.has(value.toLowerCase())) value = `${index++}-${safe}`;
    used.add(value.toLowerCase());
    return value;
  });
}
export const MAX_PIXELS = 2 * 1024 * 1024;
/** Validate bounds before creating a browser preview, which also decodes PNG. */
export function inspectPng(bytes: Uint8Array) {
  if (bytes.length > MAX_FILE_BYTES) throw new Error("超过 16 MiB 文件上限");
  if (
    bytes.length < 33 ||
    ![137, 80, 78, 71, 13, 10, 26, 10].every((v, i) => bytes[i] === v)
  )
    throw new Error("不是有效的 PNG 文件");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint32(8) !== 13 || view.getUint32(12) !== 0x49484452)
    throw new Error("PNG 头部无效");
  const width = view.getUint32(16),
    height = view.getUint32(20);
  if (!width || !height || width * height > MAX_PIXELS)
    throw new Error("超过浏览器 2 MP 像素上限，请使用原生 CLI");
  let offset = 8,
    hasData = false,
    ended = false;
  while (offset + 12 <= bytes.length) {
    const length = view.getUint32(offset),
      end = offset + length + 12;
    if (end > bytes.length) throw new Error("PNG 文件不完整");
    const type = view.getUint32(offset + 4);
    if (type === 0x6163544c) throw new Error("暂不支持 APNG 动画");
    if (type === 0x49444154) hasData = true;
    if (type === 0x49454e44) {
      ended = true;
      break;
    }
    offset = end;
  }
  if (!hasData || !ended) throw new Error("PNG 文件不完整");
  return { width, height };
}
