import ignore from "ignore";
export interface ProjectFile {
  path: string;
  file: File;
}
const EXT = /\.(png|jpe?g|heic|heif)$/i;
const LIMIT = 50000;
export function cleanPath(path: string) {
  const parts = path.replaceAll("\\", "/").split("/");
  if (parts.some((p) => !p || p === "." || p === ".."))
    throw Error("目录路径无效");
  return parts.join("/");
}
/** Nested .gitignore rules follow traversal order; ignored parents are never re-entered. */
export async function selectImages(files: ProjectFile[]) {
  if (files.length > LIMIT)
    throw Error("目录超过 50,000 个文件，请选择资源子目录");
  const rules = new Map<string, ReturnType<typeof ignore>>();
  for (const f of files) {
    f.path = cleanPath(f.path);
    if (f.path.endsWith("/.gitignore") || f.path === ".gitignore") {
      if (f.file.size > 1024 * 1024) throw Error(".gitignore 过大");
      rules.set(
        f.path.slice(0, -".gitignore".length),
        ignore().add(await f.file.text()),
      );
    }
  }
  let excluded = 0;
  const images = files.filter((f) => {
    if (!EXT.test(f.path)) return false;
    const parts = f.path.split("/");
    if (parts.some((p) => p === ".git" || p === ".svn")) {
      excluded++;
      return false;
    }
    let ignored = false;
    for (let end = 1; end <= parts.length; end++) {
      const current =
        parts.slice(0, end).join("/") + (end < parts.length ? "/" : "");
      for (let depth = 0; depth < end; depth++) {
        const base = parts.slice(0, depth).join("/") + (depth ? "/" : "");
        const rule = rules.get(base);
        if (!rule) continue;
        const test = rule.test(current.slice(base.length));
        if (test.ignored) ignored = true;
        else if (test.unignored) ignored = false;
      }
      if (ignored) {
        excluded++;
        return false;
      }
    }
    return true;
  });
  if (images.length > 5000) throw Error("超过 5,000 张图片，请选择资源子目录");
  return { images, excluded };
}
export async function droppedFiles(
  items: DataTransferItemList,
): Promise<ProjectFile[]> {
  const roots = Array.from(items)
    .map((i) => i.webkitGetAsEntry())
    .filter(Boolean) as FileSystemEntry[];
  const result: ProjectFile[] = [];
  async function visit(entry: FileSystemEntry, prefix: string) {
    if (entry.name === ".git" || entry.name === ".svn") return;
    const path = prefix + entry.name;
    if (entry.isFile) {
      const file = await new Promise<File>((resolve, reject) =>
        (entry as FileSystemFileEntry).file(resolve, reject),
      );
      result.push({ path, file });
      if (result.length > LIMIT)
        throw Error("目录超过 50,000 个文件，请选择资源子目录");
    } else {
      const reader = (entry as FileSystemDirectoryEntry).createReader();
      for (;;) {
        const entries = await new Promise<FileSystemEntry[]>(
          (resolve, reject) => reader.readEntries(resolve, reject),
        );
        if (!entries.length) break;
        for (const child of entries) await visit(child, path + "/");
      }
    }
  }
  for (const root of roots) await visit(root, "");
  return result;
}
export function outputPath(path: string, format: string, used: Set<string>) {
  const base = cleanPath(path).replace(
    /\.(png|jpe?g|heic|heif)$/i,
    "." + format,
  );
  let name = base,
    n = 2;
  while (used.has(name.toLowerCase()))
    name = base.replace(/\.[^.]+$/, `-${n++}.${format}`);
  used.add(name.toLowerCase());
  return name;
}
