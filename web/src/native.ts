import { zipSync, strToU8 } from "fflate";
import { size } from "./helpers";
import {
  selectImages,
  droppedFiles,
  outputPath,
  type ProjectFile,
} from "./project";
interface Result {
  improved: boolean;
  message?: string;
  format: string;
  quality: number;
  width: number;
  height: number;
  original_bytes: number;
  optimized_bytes: number;
  saved_bytes: number;
  transparent_pixels: number;
  difference?: { ssimulacra2?: number; max_alpha_error: number };
  bytes?: string;
  originalPreview?: string;
  candidatePreview?: string;
}
interface Row extends ProjectFile {
  state: string;
  result?: Result;
  blob?: Blob;
  error?: string;
}
const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;
let rows: Row[] = [],
  selected: Row | undefined,
  running = false,
  reading = false,
  paused = false,
  available = false,
  exporting = false;
let previewURLs: string[] = [],
  downloadURL: string | undefined;
const theme = $<HTMLSelectElement>("theme");
try {
  theme.value = localStorage.getItem("resopt-web-theme") || "system";
} catch {}
const media = matchMedia("(prefers-color-scheme:dark)");
function applyTheme() {
  document.documentElement.dataset.theme =
    theme.value === "system" ? (media.matches ? "dark" : "light") : theme.value;
}
theme.onchange = () => {
  applyTheme();
  try {
    localStorage.setItem("resopt-web-theme", theme.value);
  } catch {}
};
media.addEventListener("change", applyTheme);
applyTheme();
function say(message: string) {
  $("status").textContent = message;
}
function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  text = "",
  className = "",
) {
  const node = document.createElement(tag);
  node.textContent = text;
  node.className = className;
  return node;
}
function b64(value: string, mime: string) {
  const bytes = Uint8Array.from(atob(value), (c) => c.charCodeAt(0));
  return new Blob([bytes], { type: mime });
}
function save(blob: Blob, name: string) {
  if (downloadURL) URL.revokeObjectURL(downloadURL);
  downloadURL = URL.createObjectURL(blob);
  const a = el("a", `下载 ${name}`, "download-link");
  a.href = downloadURL;
  a.download = name;
  $("status").replaceChildren(a);
  a.click();
}
function totals() {
  $("count").textContent = String(rows.length);
  $("original-total").textContent = size(
    rows.reduce((n, r) => n + r.file.size, 0),
  );
  $("saved-total").textContent = size(
    rows.reduce((n, r) => n + (r.result?.saved_bytes || 0), 0),
  );
  for (const id of ["choose", "choose-project", "clear"])
    $<HTMLButtonElement>(id).disabled = running || reading || exporting;
  $<HTMLButtonElement>("start").disabled =
    !available ||
    running ||
    reading ||
    exporting ||
    !rows.some((r) => r.state === "待分析");
  $("start").hidden = running;
  $("pause").hidden = !running;
  $<HTMLButtonElement>("pause").disabled = paused;
  $<HTMLButtonElement>("download-all").disabled =
    running || reading || exporting || !rows.some((r) => r.result);
  for (const id of ["format", "quality"])
    $<HTMLSelectElement>(id).disabled = running;
  $("empty").hidden = rows.length > 0;
  $("progress").style.width =
    `${rows.length ? (100 * rows.filter((r) => !["待分析", "处理中"].includes(r.state)).length) / rows.length : 0}%`;
}
function renderList() {
  const list = $("results");
  list.replaceChildren();
  const query = $<HTMLInputElement>("search").value.toLowerCase();
  const matched = rows
    .filter((r) => r.path.toLowerCase().includes(query))
    .sort(
      (a, b) => (b.result?.saved_bytes || 0) - (a.result?.saved_bytes || 0),
    );
  for (const r of matched.slice(0, 200)) {
    const button = el("button", "", "row");
    button.type = "button";
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", String(selected === r));
    button.title = r.path;
    const copy = el("span", "", "file-copy");
    copy.append(
      el("span", r.path, "filename"),
      el("span", r.error || r.state, "file-note"),
    );
    button.append(
      copy,
      el("span", size(r.file.size), "num"),
      el(
        "span",
        r.result?.saved_bytes ? size(r.result.saved_bytes) : r.state,
        "num",
      ),
    );
    button.onclick = () => {
      selected = r;
      renderList();
      renderDetail();
    };
    list.append(button);
  }
  if (matched.length > 200)
    list.append(
      el(
        "p",
        `显示收益最高的 200 / ${matched.length} 张；搜索路径可定位其它图片。`,
        "detail-note",
      ),
    );
  totals();
}
function renderDetail() {
  for (const url of previewURLs) URL.revokeObjectURL(url);
  previewURLs = [];
  const pane = $("inspector");
  pane.replaceChildren();
  const r = selected;
  if (!r) {
    pane.append(
      el("h2", "先看整体收益，再检查重点图片"),
      el("p", "支持项目目录拖拽和批量分析，列表按实际节省大小排序。"),
    );
    return;
  }
  pane.append(
    el("h2", r.path),
    el("p", r.error || r.result?.message || r.state, "detail-note"),
  );
  if (!r.result?.improved) return;
  const result = r.result,
    comparison = el("div", "", "comparison");
  for (const [title, value] of [
    ["原图", result.originalPreview],
    ["优化候选", result.candidatePreview],
  ]) {
    const figure = el("figure", "", "preview"),
      canvas = el("div", "", "canvas");
    canvas.dataset.background = "checker";
    const img = el("img");
    const url = URL.createObjectURL(b64(value!, "image/png"));
    previewURLs.push(url);
    img.src = url;
    img.alt = title!;
    canvas.append(img);
    figure.append(el("figcaption", title!), canvas);
    comparison.append(figure);
  }
  pane.append(
    comparison,
    el(
      "p",
      `${result.width} × ${result.height} · ${result.format.toUpperCase()} / ${result.quality ?? "无损"} · ${size(result.original_bytes)} → ${size(result.optimized_bytes)}`,
      "facts",
    ),
    el(
      "p",
      `SSIMULACRA2：${result.difference?.ssimulacra2?.toFixed(2) ?? "—"}；最大 Alpha 误差：${result.difference?.max_alpha_error?.toFixed(4) ?? "—"}`,
      "facts",
    ),
    el(
      "p",
      `${result.quality ? "有损候选，请检查画质后下载。" : "PNG 无损候选。"}跨格式替换需要更新 Contents.json、代码或工程引用；此页面不会修改项目。`,
      "detail-note",
    ),
  );
  const download = el("button", "下载此候选", "primary");
  download.onclick = () =>
    save(
      r.blob!,
      outputPath(r.path, result.format, new Set()).split("/").pop()!,
    );
  pane.append(download);
}
async function add(files: ProjectFile[]) {
  const { images, excluded } = await selectImages(files);
  const seen = new Set(rows.map((r) => r.path));
  let skipped = 0;
  if (rows.length + images.length > 5000)
    throw Error("最多 5,000 张图片，请分批选择");
  for (const entry of images) {
    if (seen.has(entry.path)) continue;
    seen.add(entry.path);
    const oversize = entry.file.size > 16 * 1024 * 1024;
    skipped += Number(oversize);
    rows.push({
      ...entry,
      state: oversize ? "未处理" : "待分析",
      error: oversize ? "超过 16 MiB 文件上限" : undefined,
    });
  }
  selected ??= rows[0];
  renderList();
  renderDetail();
  say(
    `已收集 ${rows.length} 张图片，忽略 ${excluded} 张，超限 ${skipped} 张。点击“上传并分析”后，仅图片将传给本机进程，不离开设备。`,
  );
}
async function collect(read: () => Promise<ProjectFile[]>) {
  if (running || reading || exporting) return;
  reading = true;
  totals();
  say("正在读取目录与 .gitignore…");
  try {
    await add(await read());
  } catch (error) {
    say(error instanceof Error ? error.message : String(error));
  } finally {
    reading = false;
    totals();
  }
}
$("choose").onclick = () => $<HTMLInputElement>("files").click();
$("empty-choose").onclick = () => $<HTMLInputElement>("project").click();
$("choose-project").onclick = () => $<HTMLInputElement>("project").click();
for (const id of ["files", "project"])
  $(id).onchange = () => {
    const input = $<HTMLInputElement>(id);
    const files = Array.from(input.files || []).map((file) => ({
      file,
      path: file.webkitRelativePath || file.name,
    }));
    input.value = "";
    void collect(async () => files);
  };
const drop = $("drop-area");
drop.ondragover = (e) => {
  e.preventDefault();
  drop.classList.add("dragover");
};
drop.ondragleave = () => drop.classList.remove("dragover");
drop.ondrop = (e) => {
  e.preventDefault();
  drop.classList.remove("dragover");
  const items = e.dataTransfer?.items;
  if (items) void collect(() => droppedFiles(items));
};
$("search").oninput = renderList;
$("clear").onclick = () => {
  rows = [];
  selected = undefined;
  if (downloadURL) URL.revokeObjectURL(downloadURL);
  downloadURL = undefined;
  renderList();
  renderDetail();
  say("已清空。");
};
$("pause").onclick = () => {
  paused = true;
  totals();
  say("将在当前图片完成后暂停，已完成结果保留。");
};
$("start").onclick = async () => {
  if (running || reading || !available) return;
  running = true;
  paused = false;
  const format = $<HTMLSelectElement>("format").value,
    quality = $<HTMLSelectElement>("quality").value;
  for (const r of rows.filter((r) => r.state === "待分析")) {
    if (paused) break;
    r.state = "处理中";
    renderList();
    say(`正在分析 ${r.path}…`);
    try {
      const response = await fetch(
        `./api/convert?format=${format}&quality=${quality}`,
        {
          method: "POST",
          headers: {
            "Content-Type": "application/octet-stream",
            "X-Resopt-Request": "1",
          },
          body: r.file,
        },
      );
      const result = await response.json();
      if (response.status === 429) {
        r.state = "待分析";
        paused = true;
        say("服务器忙，已暂停；稍后点击“上传并分析”继续。");
        break;
      }
      if (!response.ok) throw Error(result.error || "处理失败");
      if (result.improved) {
        const blob = b64(
          result.bytes,
          result.format === "heic"
            ? "image/heic"
            : result.format === "png"
              ? "image/png"
              : "image/jpeg",
        );
        const cacheBytes = rows.reduce(
          (n, x) =>
            n +
            (x.blob?.size || 0) +
            (x.result?.originalPreview?.length || 0) +
            (x.result?.candidatePreview?.length || 0),
          0,
        );
        if (
          cacheBytes +
            blob.size +
            (result.originalPreview?.length || 0) +
            (result.candidatePreview?.length || 0) >
          128 * 1024 * 1024
        ) {
          r.state = "待分析";
          paused = true;
          say("结果缓存已达到 128 MiB，请下载并清空后分批处理。");
          break;
        }
        r.blob = blob;
        delete result.bytes;
      }
      r.result = result;
      r.state = result.improved ? "可优化" : "无收益";
    } catch (error) {
      r.state = "未处理";
      r.error = error instanceof Error ? error.message : String(error);
    }
    renderList();
    if (selected === r) renderDetail();
  }
  running = false;
  renderList();
  renderDetail();
  if (!paused)
    say(
      `分析完成：${rows.filter((r) => r.result?.improved).length} 张可优化，合计节省 ${size(rows.reduce((n, r) => n + (r.result?.saved_bytes || 0), 0))}。结果仅为候选，原项目未改动。`,
    );
};
$("download-all").onclick = async () => {
  exporting = true;
  totals();
  try {
    const archive: Record<string, Uint8Array> = Object.create(null),
      used = new Set<string>(["report.json"]);
    const report = [];
    for (const r of rows) {
      let output: string | undefined;
      if (r.blob) {
        output = outputPath(r.path, r.result!.format, used);
        archive[output] = new Uint8Array(await r.blob.arrayBuffer());
      }
      const { originalPreview, candidatePreview, bytes, ...summary } =
        r.result || {};
      report.push({
        source: r.path,
        output,
        state: r.state,
        error: r.error,
        ...summary,
      });
    }
    archive["report.json"] = strToU8(
      JSON.stringify(
        { tool: "resopt-macos-web", references_updated: false, files: report },
        null,
        2,
      ),
    );
    const bytes = zipSync(archive, { level: 0 });
    save(
      new Blob([bytes.slice().buffer as ArrayBuffer], {
        type: "application/zip",
      }),
      "resopt-project-candidates.zip",
    );
  } catch {
    say("导出失败，请缩小批次");
  } finally {
    exporting = false;
    totals();
  }
};
void fetch("./api/capabilities")
  .then((r) => r.json())
  .then((c) => {
    available = c.native === true;
    totals();
    say(
      available
        ? "服务就绪。拖入项目后，先在本地筛选图片，再决定上传分析。"
        : "此站点没有 macOS 后端",
    );
  })
  .catch(() =>
    say("此页面需要本机 macOS 进程。推荐运行 resopt web /path/to/project。"),
  );
