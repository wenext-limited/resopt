import { zipSync, strToU8 } from "fflate";
import { Engine } from "./engine";
import {
  size,
  exportNames,
  MAX_FILES,
  MAX_FILE_BYTES,
  MAX_OUTPUT_BYTES,
  inspectPng,
} from "./helpers";
import type { Summary } from "./types";

type Row = {
  id: number;
  file: File;
  state: "queued" | "working" | "done" | "error";
  originalURL: string;
  resultURL?: string;
  result?: Blob;
  summary?: Summary;
  error?: string;
};
const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;
const engine = new Engine();
let rows: Row[] = [],
  selected: number | null = null,
  nextId = 0,
  running = false,
  adding = false,
  ready = false,
  runId = 0,
  background = "checker",
  outputBytes = 0;
let downloadURL: string | null = null;
const labels = {
  queued: "待优化",
  working: "处理中…",
  done: "已完成",
  error: "未处理",
};
function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls = "",
  text?: string,
) {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}
function say(text: string) {
  $("status").textContent = text;
}
function theme() {
  const select = $<HTMLSelectElement>("theme"),
    system = matchMedia("(prefers-color-scheme:dark)");
  try {
    const saved = localStorage.getItem("resopt-web-theme");
    if (saved && ["system", "light", "dark"].includes(saved))
      select.value = saved;
  } catch {}
  const apply = () => {
    document.documentElement.dataset.theme =
      select.value === "system"
        ? system.matches
          ? "dark"
          : "light"
        : select.value;
  };
  select.addEventListener("change", () => {
    apply();
    try {
      localStorage.setItem("resopt-web-theme", select.value);
    } catch {}
  });
  system.addEventListener("change", apply);
  apply();
}
theme();
function totals() {
  $("count").textContent = String(rows.length);
  $("original-total").textContent = size(
    rows.reduce((n, r) => n + r.file.size, 0),
  );
  $("saved-total").textContent = size(
    rows.reduce((n, r) => n + (r.summary?.saved_bytes || 0), 0),
  );
  $<HTMLButtonElement>("start").disabled =
    !ready || running || adding || !rows.some((r) => r.state === "queued");
  $("start").hidden = running;
  $("pause").hidden = !running;
  $<HTMLButtonElement>("choose").disabled = running || adding;
  $<HTMLButtonElement>("clear").disabled = !rows.length || running || adding;
  $<HTMLButtonElement>("download-all").disabled =
    running || !rows.some((r) => r.state === "done");
  $<HTMLSelectElement>("effort").disabled = running;
  $<HTMLInputElement>("reductions").disabled = running;
  const finished = rows.filter(
    (r) => r.state === "done" || r.state === "error",
  ).length;
  $("progress").style.width =
    `${rows.length ? (100 * finished) / rows.length : 0}%`;
  $("empty").hidden = rows.length > 0;
}
function renderList() {
  const list = $("results");
  list.replaceChildren();
  const query = $<HTMLInputElement>("search").value.trim().toLocaleLowerCase();
  for (const r of rows.filter((r) =>
    r.file.name.toLocaleLowerCase().includes(query),
  )) {
    const button = node("button", "row");
    button.type = "button";
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", String(r.id === selected));
    button.title = r.file.name;
    const identity = node("span", "identity"),
      img = node("img");
    img.src = r.originalURL;
    img.alt = "";
    img.loading = "lazy";
    const copy = node("span", "file-copy");
    copy.append(
      node("span", "filename", r.file.name),
      node(
        "span",
        "file-note",
        r.summary ? `${r.summary.width} × ${r.summary.height}` : "PNG",
      ),
    );
    identity.append(img, copy);
    const saved = node(
      "span",
      `num ${r.state === "error" ? "error" : r.summary?.saved_bytes ? "accent" : ""}`,
      r.state === "done" ? size(r.summary?.saved_bytes || 0) : labels[r.state],
    );
    if (r.summary)
      saved.append(
        node(
          "small",
          "",
          r.summary.saved_bytes
            ? `−${((100 * r.summary.saved_bytes) / r.file.size).toFixed(1)}%`
            : "已是较小编码",
        ),
      );
    button.append(identity, node("span", "num", size(r.file.size)), saved);
    button.addEventListener("click", () => {
      selected = r.id;
      renderList();
      renderDetail();
    });
    list.append(button);
  }
  if (rows.length && !list.children.length)
    list.append(node("p", "detail-note", "没有匹配的图片。"));
  totals();
}
function preview(r: Row, result: boolean) {
  const f = node("figure", "preview");
  f.append(node("div", "preview-label", result ? "优化结果" : "原图"));
  const canvas = node("div", "canvas");
  canvas.dataset.background = background;
  const url = result ? r.resultURL : r.originalURL;
  if (url) {
    const img = node("img");
    img.src = url;
    img.alt = `${r.file.name} ${result ? "优化结果" : "原图"}`;
    canvas.append(img);
  } else
    canvas.append(
      node(
        "span",
        "placeholder",
        r.state === "working" ? "正在优化…" : "等待优化",
      ),
    );
  f.append(
    canvas,
    node(
      "figcaption",
      "",
      result
        ? r.summary
          ? size(r.summary.optimized_bytes)
          : "—"
        : size(r.file.size),
    ),
  );
  return f;
}
function save(blob: Blob, name: string) {
  if (downloadURL) URL.revokeObjectURL(downloadURL);
  downloadURL = URL.createObjectURL(blob);
  const a = node("a", "download-link", `下载 ${name}`);
  a.href = downloadURL;
  a.download = name;
  $("status").replaceChildren(node("span", "", "文件已就绪："), a);
  a.click();
}
function renderDetail() {
  const pane = $("inspector"),
    r = rows.find((r) => r.id === selected);
  pane.replaceChildren();
  if (!r) {
    const empty = node("div", "empty");
    empty.append(
      node("h2", "", "原图与结果，一目了然"),
      node("p", "", "选择图片后可查看透明背景、像素验证和体积变化。"),
    );
    pane.append(empty);
    return;
  }
  const heading = node("div", "detail-heading"),
    title = node("div");
  title.append(node("div", "eyebrow", "PNG"), node("h2", "", r.file.name));
  heading.append(title, node("span", "tag", labels[r.state]));
  pane.append(heading);
  const facts = node("div", "facts");
  if (r.summary)
    facts.append(
      node("span", "", `${r.summary.width} × ${r.summary.height}`),
      node(
        "span",
        "",
        r.summary.transparent_pixels ? "含透明像素" : "无透明像素",
      ),
      node(
        "span",
        "",
        r.summary.reductions ? "无损颜色精简已启用" : "严格无损模式",
      ),
    );
  else facts.append(node("span", "", "原文件保留在你的设备上"));
  pane.append(facts);
  const backgrounds = node("div", "backgrounds");
  backgrounds.setAttribute("role", "group");
  backgrounds.setAttribute("aria-label", "预览背景");
  for (const [value, label] of [
    ["checker", "棋盘背景"],
    ["light", "白色背景"],
    ["dark", "深色背景"],
  ]) {
    const b = node("button", "swatch");
    b.type = "button";
    b.dataset.background = value;
    b.setAttribute("aria-label", label);
    b.setAttribute("aria-pressed", String(background === value));
    b.addEventListener("click", () => {
      background = value;
      renderDetail();
    });
    backgrounds.append(b);
  }
  pane.append(backgrounds);
  const comparison = node("div", "comparison");
  comparison.append(preview(r, false), preview(r, true));
  pane.append(comparison);
  if (r.summary) {
    const metrics = node("div", "metrics");
    for (const [label, value] of [
      ["像素校验", r.summary.pixel_equivalent ? "完全一致" : "未通过"],
      ["SSIMULACRA2", r.summary.ssimulacra2.toFixed(2)],
      ["实际节省", size(r.summary.saved_bytes)],
    ]) {
      const cell = node("div");
      cell.append(node("span", "", label), node("strong", "", value));
      metrics.append(cell);
    }
    pane.append(metrics);
  }
  const actions = node("div", "detail-actions");
  if (r.result) {
    const download = node("button", "primary", "下载优化图片");
    download.addEventListener("click", () =>
      save(r.result!, exportNames([r.file.name])[0]),
    );
    actions.append(download);
  }
  const remove = node("button", "", "移除此项");
  remove.disabled = running;
  remove.addEventListener("click", () => {
    URL.revokeObjectURL(r.originalURL);
    if (r.resultURL) URL.revokeObjectURL(r.resultURL);
    outputBytes -= r.result?.size || 0;
    rows = rows.filter((x) => x !== r);
    selected = rows[0]?.id ?? null;
    renderList();
    renderDetail();
  });
  actions.append(remove);
  pane.append(actions);
  pane.append(
    node(
      "p",
      r.error ? "detail-note error" : "detail-note",
      r.error ||
        "校验逐个比较解码后的 RGBA 像素，包括完全透明像素中的 RGB。下载文件不会覆盖原图。",
    ),
  );
}
async function addFiles(files: File[]) {
  if (running || adding) {
    say("请等待图片检查完成，或先暂停优化。");
    return;
  }
  if (rows.length + files.length > MAX_FILES) {
    say(`单次最多 ${MAX_FILES} 张图片，请分批处理。`);
    return;
  }
  adding = true;
  say("正在检查图片…");
  totals();
  let rejected = 0;
  let firstError = "";
  for (const file of files) {
    try {
      if (file.size > MAX_FILE_BYTES) throw new Error("超过 16 MiB 文件上限");
      inspectPng(new Uint8Array(await file.arrayBuffer()));
    } catch (error) {
      rejected++;
      firstError ||= `${file.name}：${error instanceof Error ? error.message : String(error)}`;
      continue;
    }
    rows.push({
      id: ++nextId,
      file,
      state: "queued",
      originalURL: URL.createObjectURL(new Blob([file], { type: "image/png" })),
    });
  }
  adding = false;
  selected ??= rows[0]?.id ?? null;
  renderList();
  renderDetail();
  say(
    `已添加 ${files.length - rejected} 张图片${rejected ? `；${rejected} 个文件无法处理。${firstError}` : ""}。选择压缩设置后开始。`,
  );
}
function choose() {
  if (!running && !adding) $<HTMLInputElement>("files").click();
}
$("choose").addEventListener("click", choose);
$("empty-choose").addEventListener("click", choose);
$("files").addEventListener("change", async () => {
  const input = $<HTMLInputElement>("files");
  const files = Array.from(input.files || []);
  input.value = "";
  await addFiles(files);
});
const drop = $("drop-area");
drop.addEventListener("dragover", (event) => {
  event.preventDefault();
  if (!running) drop.classList.add("dragover");
});
drop.addEventListener("dragleave", () => drop.classList.remove("dragover"));
drop.addEventListener("drop", (event) => {
  event.preventDefault();
  drop.classList.remove("dragover");
  void addFiles(Array.from(event.dataTransfer?.files || []));
});
$("search").addEventListener("input", renderList);
$("start").addEventListener("click", async () => {
  if (running || !ready) return;
  running = true;
  const token = ++runId;
  const effort = Number($<HTMLSelectElement>("effort").value),
    reductions = $<HTMLInputElement>("reductions").checked;
  renderList();
  renderDetail();
  for (const r of rows.filter((r) => r.state === "queued")) {
    if (token !== runId) break;
    r.state = "working";
    renderList();
    if (r.id === selected) renderDetail();
    say(`正在优化 ${r.file.name}…`);
    try {
      const input = await r.file.arrayBuffer();
      if (token !== runId) break;
      const response = await engine.optimize(input, effort, reductions);
      if (token !== runId) break;
      if (!response.ok || !response.bytes || !response.summary)
        throw Error("图像引擎返回不完整结果");
      if (outputBytes + response.bytes.byteLength > MAX_OUTPUT_BYTES)
        throw Error("结果缓存已达到 128 MiB，请下载并清空后分批处理。");
      r.result = new Blob([response.bytes], { type: "image/png" });
      r.resultURL = URL.createObjectURL(r.result);
      r.summary = response.summary;
      r.state = "done";
      outputBytes += r.result.size;
    } catch (error) {
      if (token !== runId) {
        break;
      }
      r.state = "error";
      r.error = error instanceof Error ? error.message : String(error);
    }
    renderList();
    if (r.id === selected) renderDetail();
  }
  if (token === runId) {
    running = false;
    renderList();
    renderDetail();
    const done = rows.filter((r) => r.state === "done").length,
      failed = rows.filter((r) => r.state === "error").length;
    say(
      `处理完成：${done} 张成功${failed ? `，${failed} 张未处理（选择图片查看原因）` : ""}。`,
    );
  }
});
$("pause").addEventListener("click", () => {
  runId++;
  running = false;
  engine.cancel();
  for (const r of rows) if (r.state === "working") r.state = "queued";
  say("已暂停，已完成的结果仍可下载。");
  renderList();
  renderDetail();
});
$("clear").addEventListener("click", () => {
  engine.cancel();
  runId++;
  for (const r of rows) {
    URL.revokeObjectURL(r.originalURL);
    if (r.resultURL) URL.revokeObjectURL(r.resultURL);
  }
  rows = [];
  selected = null;
  outputBytes = 0;
  if (downloadURL) {
    URL.revokeObjectURL(downloadURL);
    downloadURL = null;
  }
  renderList();
  renderDetail();
  say("已清空，可选择下一批 PNG。");
});
$("download-all").addEventListener("click", async () => {
  const button = $<HTMLButtonElement>("download-all");
  button.disabled = true;
  say("正在整理下载文件…");
  try {
    const done = rows.filter((r) => r.result),
      names = exportNames(done.map((r) => r.file.name));
    const files: Record<string, Uint8Array> = Object.create(null);
    for (let i = 0; i < done.length; i++)
      files[names[i]] = new Uint8Array(await done[i].result!.arrayBuffer());
    files["report.json"] = strToU8(
      JSON.stringify(
        {
          tool: "resopt-web",
          scope: "selected static PNG files",
          files: done.map((r, i) => ({
            name: r.file.name,
            output: names[i],
            ...r.summary,
          })),
          failed: rows
            .filter((r) => r.state === "error")
            .map((r) => ({ name: r.file.name, error: r.error })),
        },
        null,
        2,
      ),
    );
    const archive = zipSync(files, { level: 0 });
    save(
      new Blob([archive.slice().buffer as ArrayBuffer], {
        type: "application/zip",
      }),
      "resopt-optimized.zip",
    );
  } catch (error) {
    say(`导出失败：${error instanceof Error ? error.message : String(error)}`);
  } finally {
    totals();
  }
});
void engine
  .ready()
  .then(() => {
    ready = true;
    totals();
    say("就绪。选择或拖入静态 PNG，所有处理都在浏览器内完成。");
  })
  .catch((error) => {
    say(`图像引擎加载失败：${error.message}。请刷新重试。`);
  });
