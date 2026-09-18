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
        matches!(report.schema_version, 1 | 2),
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
    let payload = serde_json::json!({
        "sessionToken": null,
        "meta": meta(report),
        "resources": report.resources,
    });
    Ok(page(&script_safe_json(&payload)?).into_string())
}

/// The live application shell: rows arrive over the session API, so the page
/// stays small no matter how large the project is.
pub(crate) fn render_live_page(project: &Path, token: &str) -> Result<String> {
    let payload = serde_json::json!({
        "sessionToken": token,
        "meta": {"root": project},
    });
    Ok(page(&script_safe_json(&payload)?).into_string())
}

/// Report-level facts the UI needs besides the resource rows.
pub(crate) fn meta(report: &AnalysisReport) -> serde_json::Value {
    serde_json::json!({
        "root": report.root,
        "backend": report.backend,
        "options": report.options,
        "savings": report.potential_source_bytes_saved,
        "cancelled": report.cancelled,
        "similarGroups": report.similar_groups,
        "performance": report.performance,
        "projectKinds": report.inventory.project_kinds,
        "androidMinSdk": report.inventory.android_min_sdk,
        "diagnostics": report.inventory.diagnostics,
        "excludedDirectories": report.inventory.excluded_directories,
    })
}

// An inert JSON script still ends at a literal </script>. Escape HTML
// delimiters before embedding data; UI code uses textContent for filenames.
fn script_safe_json(value: &serde_json::Value) -> Result<String> {
    Ok(serde_json::to_string(value)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

use maud::{DOCTYPE, Markup, PreEscaped, html};

const THEME_BOOTSTRAP: &str = "try{const t=localStorage.getItem('resopt-theme')||'system';document.documentElement.dataset.theme=t==='system'?(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):t}catch{}";

/// The UI sources share one scope; `start()` runs after every file is defined.
fn script() -> String {
    [
        "'use strict';(() => {",
        include_str!("ui/core.js"),
        include_str!("ui/i18n.js"),
        include_str!("ui/app.js"),
        include_str!("ui/detail.js"),
        include_str!("ui/batch.js"),
        "start();})();",
    ]
    .join("\n")
}

fn page(data: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "resopt · Resource analysis" }
                script { (PreEscaped(THEME_BOOTSTRAP)) }
                style { (PreEscaped(include_str!("ui/style.css"))) }
            }
            body {
                a.skip-link href="#results" data-i18n="colResource" { "Resource" }
                header {
                    div.brand {
                        span.mark aria-hidden="true" { i {} i {} i {} i {} }
                        strong { "resopt" } span data-i18n="title" { "Resource analysis" }
                    }
                    div.header-meta {
                        span.read-only id="session-mode" {}
                        select id="language" aria-label="Language" data-i18n-label="language" {
                            option value="en" { "English" }
                            option value="zh-CN" { "简体中文" }
                        }
                        select id="theme" aria-label="Theme" data-i18n-label="theme" {
                            option value="system" data-i18n="themeSystem" { "System" }
                            option value="light" data-i18n="themeLight" { "Light" }
                            option value="dark" data-i18n="themeDark" { "Dark" }
                        }
                        a href="analysis.json" target="_blank" rel="noopener" data-i18n="json" { "View JSON ↗" }
                    }
                }
                main {
                    section.status id="status" role="status" aria-live="polite" {}
                    (overview())
                    (toolbar())
                    div.workspace {
                        section.list-pane aria-label="Resources" data-i18n-label="statResources" {
                            div.list-head aria-hidden="true" {
                                span data-i18n="colResource" {} span data-i18n="colSize" {} span data-i18n="colSavings" {}
                            }
                            div.results id="results" role="listbox" tabindex="-1" aria-label="Resources" data-i18n-label="statResources" {}
                            div.pager {
                                span id="range" role="status" aria-live="polite" {}
                                div.pager-controls {
                                    button.icon-button type="button" id="previous" aria-label="Previous page" data-i18n-label="previous" { "←" }
                                    span id="page-number" {}
                                    button.icon-button type="button" id="next" aria-label="Next page" data-i18n-label="next" { "→" }
                                }
                            }
                        }
                        aside.inspector id="inspector" aria-label="Details" {}
                    }
                    footer.footer {
                        span data-i18n="footerUnits" {}
                        span data-i18n="footerScope" {}
                    }
                }
                (dialogs())
                noscript { "Enable JavaScript to filter resources and compare images, or open analysis.json next to this file." }
                // JSON is escaped for the script context by script_safe_json, not HTML-escaped.
                script type="application/json" id="report-data" { (PreEscaped(data)) }
                script { (PreEscaped(script())) }
            }
        }
    }
}

fn overview() -> Markup {
    html! {
        section.summary aria-label="Overview" {
            @for (id, label, accent) in [
                ("stat-resources", "statResources", false),
                ("stat-opportunities", "statOpportunities", false),
                ("stat-savings", "statSavings", true),
                ("stat-warnings", "statWarnings", false),
                ("stat-applied", "statApplied", false),
            ] {
                div.stat {
                    div.stat-label data-i18n=(label) {}
                    div class={ "stat-value" @if accent { " accent" } } id=(id) { "—" }
                }
            }
            p.summary-note id="scope-note" {}
        }
    }
}

fn toolbar() -> Markup {
    html! {
        section.toolbar aria-label="Filters" {
            div.modes role="group" aria-label="View" {
                @for mode in ["candidates", "warnings", "duplicates", "applied", "images", "unsupported", "failed", "all"] {
                    button.mode type="button" data-mode=(mode) aria-pressed="false" {
                        span data-i18n={ "mode" (mode[..1].to_uppercase()) (mode[1..]) } {}
                        " " span.mode-count id={ "mode-" (mode) } {}
                    }
                }
            }
            div.search {
                svg width="15" height="15" viewBox="0 0 20 20" fill="none" aria-hidden="true" {
                    circle cx="8.5" cy="8.5" r="5.5" stroke="currentColor" stroke-width="1.5" {}
                    path d="m13 13 4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" {}
                }
                input type="search" id="search" autocomplete="off" aria-label="Search" data-i18n-label="search" data-i18n-placeholder="search";
            }
            select id="format-filter" aria-label="Format" data-i18n-label="allFormats" {}
            select id="sort" aria-label="Sort" {
                option value="savings" data-i18n="sortSavings" {}
                option value="size" data-i18n="sortSize" {}
                option value="name" data-i18n="sortName" {}
                option value="score" data-i18n="sortScore" {}
            }
            button.primary type="button" id="batch-open" hidden data-i18n="batch" {}
            button type="button" id="restore-all-open" hidden data-i18n="restoreAll" {}
        }
    }
}

fn dialogs() -> Markup {
    html! {
        dialog id="apply-dialog" aria-labelledby="apply-title" {
            h2 id="apply-title" {}
            p id="apply-description" {}
            p.hint id="apply-note" {}
            div.dialog-actions {
                button type="button" id="apply-cancel" data-i18n="cancelButton" {}
                button.primary type="button" id="apply-confirm" {}
            }
        }
        dialog.wide id="compare-dialog" aria-labelledby="compare-title" {
            div.dialog-head {
                h2 id="compare-title" data-i18n="compareTitle" {}
                button type="button" id="compare-close" data-i18n="close" {}
            }
            p id="compare-caption" {}
            input type="range" id="compare-slider" min="0" max="100" value="50" aria-label="Comparison position" data-i18n-label="compare";
            div.compare-stage id="compare-stage" data-background="checker" {}
            p.hint id="compare-note" {}
        }
        dialog id="batch-dialog" aria-labelledby="batch-title" {
            div.dialog-head {
                h2 id="batch-title" {}
                button type="button" id="batch-close" data-i18n="close" {}
            }
            div id="batch-policy-view" {
                p data-i18n="batchIntro" {}
                @for (id, label, checked) in [
                    ("batch-lossless", "batchLossless", true),
                    ("batch-lossy", "batchLossy", false),
                    ("batch-cross", "batchCross", false),
                    ("batch-alpha", "batchAlpha", false),
                    ("batch-quality", "batchQuality", false),
                ] {
                    label.check { input type="checkbox" id=(id) checked[checked]; span data-i18n=(label) {} }
                }
                label.field { span data-i18n="batchMinScore" {} input type="number" id="batch-min-score" min="0" max="100" step="1" inputmode="decimal"; }
                label.check { input type="checkbox" id="batch-scope"; span id="batch-scope-label" {} }
                p.status-warn id="batch-error" role="alert" {}
                div.dialog-actions { button.primary type="button" id="batch-preview" data-i18n="batchPreview" {} }
            }
            div id="batch-plan-view" hidden {
                p.summary-text id="batch-summary" {}
                ul.batch-list id="batch-items" {}
                div.dialog-actions {
                    button type="button" id="batch-back" data-i18n="cancelButton" {}
                    button.primary type="button" id="batch-confirm" {}
                }
            }
            div id="batch-progress-view" hidden {
                p id="batch-progress-text" role="status" aria-live="polite" {}
                progress id="batch-bar" max="1" value="0" {}
                p.status-warn id="batch-stopped" hidden data-i18n="batchStopped" {}
                h3 id="batch-failures-title" hidden data-i18n="batchFailures" {}
                ul.batch-list id="batch-failures" {}
                div.dialog-actions {
                    button type="button" id="batch-stop" data-i18n="batchStop" {}
                    button.primary type="button" id="batch-done" hidden data-i18n="close" {}
                }
            }
        }
    }
}
