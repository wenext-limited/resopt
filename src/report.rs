use crate::{
    AnalysisReport,
    filesystem::{contained_file, replace, write_new},
    resources::bounded_read,
};
use anyhow::{Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Refresh presentation only. Does not encode, change JSON, or touch artifacts.
pub fn refresh_report(directory: impl AsRef<Path>) -> Result<PathBuf> {
    let directory = fs::canonicalize(directory)?;
    let data = contained_file(&directory, Path::new("analysis.json"))?;
    let report: AnalysisReport = serde_json::from_slice(&bounded_read(&data)?)?;
    ensure!(
        report.schema_version == 1,
        "unsupported analysis schema version"
    );
    let html = render_html(&report)?;
    let output = directory.join("report.html");
    if fs::symlink_metadata(&output).is_ok() {
        contained_file(&directory, Path::new("report.html"))?;
        replace(&output, html.as_bytes())?;
    } else {
        write_new(&output, html.as_bytes())?;
    }
    Ok(output)
}

pub(crate) fn render_html(report: &AnalysisReport) -> Result<String> {
    render_page(report, None)
}

pub(crate) fn render_page(report: &AnalysisReport, token: Option<&str>) -> Result<String> {
    let payload = serde_json::to_string(&serde_json::json!({
        "root": report.root,
        "sessionToken": token,
        "options": report.options,
        "resources": report.resources,
        "savings": report.potential_source_bytes_saved,
        "diagnostics": report.inventory.diagnostics,
        "excludedDirectories": report.inventory.excluded_directories,
    }))?;
    // An inert JSON script still ends at a literal </script>. Escape HTML
    // delimiters before embedding data; UI code uses textContent for filenames.
    let safe = payload
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(page(&safe, token.is_some(), &report.backend).into_string())
}

use maud::{DOCTYPE, Markup, PreEscaped, html};

fn page(data: &str, live: bool, backend: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "resopt · 资源分析" }
                script { (PreEscaped("try{const t=localStorage.getItem('resopt-theme')||'system';document.documentElement.dataset.theme=t==='system'?(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):t}catch{}")) }
                style { (PreEscaped(include_str!("report.css"))) }
            }
            body {
                header {
                    div.brand {
                        span.mark aria-hidden="true" { i {} i {} i {} i {} }
                        strong { "resopt" } span { "资源分析" }
                    }
                    div.header-meta {
                        label.sr-only for="theme" { "页面主题" }
                        select id="theme" aria-label="页面主题" {
                            option value="system" { "跟随系统" }
                            option value="light" { "浅色" }
                            option value="dark" { "深色" }
                        }
                        span.read-only { @if live { "本地服务 · 逐张确认应用" } @else { "离线报告 · 仅供审阅" } }
                        a href="analysis.json" target="_blank" rel="noopener" { "查看 JSON ↗" }
                    }
                }
                main {
                    p { "本地分析引擎：" (backend) }
                    (overview())
                    (toolbar())
                    div.workspace {
                        section.list-pane aria-label="资源清单" {
                            div.list-head aria-hidden="true" { span { "资源" } span { "原始体积" } span { "可节省" } }
                            div.results id="results" role="listbox" aria-label="资源列表" {}
                            div.pager {
                                span id="range" role="status" aria-live="polite" {}
                                div.pager-controls {
                                    button.icon-button id="previous" aria-label="上一页" { "←" }
                                    span id="page-number" {}
                                    button.icon-button id="next" aria-label="下一页" { "→" }
                                }
                            }
                        }
                        aside.inspector id="inspector" aria-label="资源详情" {}
                    }
                    footer.footer {
                        span { "体积采用 KiB / MiB（1024 进制），悬停可查看精确字节数。" }
                        span { "仅统计源文件收益 · 不等于 App 包体收益" }
                    }
                }
                dialog id="apply-dialog" aria-labelledby="apply-title" {
                    h2 id="apply-title" { "确认优化图片" }
                    p id="apply-description" {}
                    p id="apply-note" { "原文件会备份，可在页面恢复。请先检查原尺寸候选的画质。" }
                    div.dialog-actions {
                        button id="apply-cancel" { "取消" }
                        button.primary id="apply-confirm" { "确认并应用" }
                    }
                }
                noscript { "请启用 JavaScript 查看筛选和图片对比，或打开同目录的 analysis.json。" }
                // JSON is escaped for the script context by render_page, not HTML-escaped.
                script type="application/json" id="report-data" { (PreEscaped(data)) }
                script { (PreEscaped(include_str!("report.js"))) }
            }
        }
    }
}

fn overview() -> Markup {
    html! {
        section.summary aria-label="分析概览" {
            @for (id, label) in [("total-count", "资源文件"), ("candidate-count", "有更小候选"), ("total-savings", "预估可节省")] {
                div.stat {
                    div.stat-label { (label) }
                    div class={ "stat-value" @if id == "total-savings" { " accent" } } id=(id) { "—" }
                }
            }
            p.summary-note id="scope-note" {}
        }
    }
}

fn toolbar() -> Markup {
    html! {
        section.toolbar aria-label="筛选资源" {
            div.modes role="group" aria-label="资源范围" {
                @for (mode, label) in [("candidates", "有候选"), ("images", "图片"), ("all", "全部")] {
                    button class={ "mode" @if mode == "candidates" { " active" } } data-mode=(mode) aria-pressed=(if mode == "candidates" { "true" } else { "false" }) {
                        (label) " " span id={ "mode-" (mode) } {}
                    }
                }
            }
            div.search {
                svg width="15" height="15" viewBox="0 0 20 20" fill="none" aria-hidden="true" {
                    circle cx="8.5" cy="8.5" r="5.5" stroke="currentColor" stroke-width="1.5" {}
                    path d="m13 13 4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" {}
                }
                label.sr-only for="search" { "搜索资源" }
                input type="search" id="search" placeholder="搜索文件名或路径…" autocomplete="off";
            }
            label.sr-only for="format-filter" { "原始格式" }
            select id="format-filter" { option value="all" { "全部格式" } }
            label.sr-only for="sort" { "排序方式" }
            select id="sort" {
                option value="savings" { "节省量 ↓" }
                option value="size" { "原始体积 ↓" }
                option value="name" { "文件名 A–Z" }
            }
        }
    }
}
