//! Local project companion: filesystem analysis and review, never an upload API.
use crate::{AnalysisOptions, analyze_with_progress, server};
use anyhow::{Context, Result, ensure};
use maud::{DOCTYPE, PreEscaped, html};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tiny_http::{Method, Server};

#[derive(Default)]
pub struct WebOptions {
    pub out: Option<PathBuf>,
    pub port: u16,
    pub no_open: bool,
    pub analysis: AnalysisOptions,
}
#[derive(Default, Serialize)]
struct Progress {
    completed: usize,
    total: usize,
    done: bool,
    error: Option<String>,
}

/// Start a local-only project analysis and transition to the existing review UI.
/// Generated reports and restore backups are retained after the process exits.
pub fn web(root: impl AsRef<Path>, options: WebOptions) -> Result<()> {
    let root = fs::canonicalize(root).context("project directory not found")?;
    ensure!(root.is_dir(), "project must be a directory");
    options.analysis.validate()?;
    let out = match options.out {
        Some(path) => {
            let parent = fs::canonicalize(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?;
            parent.join(path.file_name().context("report directory has no name")?)
        }
        None => tempfile::Builder::new()
            .prefix("resopt-web-")
            .tempdir()?
            .keep()
            .join("analysis"),
    };
    ensure!(
        !out.starts_with(&root),
        "analysis output must be outside the scanned project"
    );
    ensure!(
        !out.try_exists()?,
        "analysis output must be a new directory"
    );
    let server = Server::http(("127.0.0.1", options.port)).map_err(|e| anyhow::anyhow!("{e}"))?;
    let address = server.server_addr().to_string();
    let origin = format!("http://{address}");
    let page = progress_page(&root, &out);
    println!(
        "Local web: {origin}/\nProject: {}\nReport: {}\nStop with Ctrl-C. Files stay on this device; reports and restore backups are retained.",
        root.display(),
        out.display()
    );
    std::io::stdout().flush()?;
    let state = Arc::new(Mutex::new(Progress::default()));
    let worker_state = state.clone();
    let worker_out = out.clone();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            analyze_with_progress(&root, &worker_out, options.analysis, |completed, total| {
                let mut state = worker_state.lock().unwrap();
                state.completed = state.completed.max(completed);
                state.total = total;
            })
        }));
        let mut state = worker_state.lock().unwrap();
        match result {
            Ok(Ok(_)) => state.done = true,
            Ok(Err(error)) => state.error = Some(format!("{error:#}")),
            Err(_) => state.error = Some("分析进程异常；请查看终端并重试。".into()),
        }
    });
    if !options.no_open {
        open_browser(&origin);
    }
    loop {
        if state.lock().unwrap().done {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("analysis thread failed"))?;
            return server::serve_on(&out, server);
        }
        let Some(request) = server.recv_timeout(Duration::from_millis(200))? else {
            continue;
        };
        if server::header(&request, "Host") != Some(address.as_str()) {
            server::respond(request, 403, "text/plain", b"Invalid host".to_vec());
            continue;
        }
        if request.method() != &Method::Get {
            server::respond(
                request,
                405,
                "text/plain",
                b"No uploads; analysis reads the selected local project".to_vec(),
            );
            continue;
        }
        match request.url() {
            "/" => server::respond(
                request,
                200,
                "text/html; charset=utf-8",
                page.as_bytes().to_vec(),
            ),
            "/api/progress" => {
                let progress = state.lock().unwrap();
                server::respond(
                    request,
                    200,
                    "application/json",
                    serde_json::to_vec(&*progress)?,
                );
            }
            _ => server::respond(request, 404, "text/plain", b"Not found".to_vec()),
        }
    }
}
fn open_browser(origin: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", ""]);
        cmd
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = Command::new("xdg-open");
    match cmd
        .arg(origin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => eprintln!("Could not open browser ({error}); open {origin}/ manually."),
    }
}
fn progress_page(root: &Path, out: &Path) -> String {
    html! {
        (DOCTYPE) html lang="zh-CN" {
            head {
                meta charset="utf-8";meta name="viewport" content="width=device-width,initial-scale=1";
                title {"resopt · 本地项目分析"}
                style {(PreEscaped(include_str!("report.css")))}
                script {(PreEscaped("try{const t=localStorage.getItem('resopt-theme')||'system';document.documentElement.dataset.theme=t==='system'?(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):t}catch{}"))}
            }
            body {main {
                h1 {"正在分析本地项目"}
                p {(root.display())}
                p {"图片直接从本机磁盘读取，不上传到远程服务器。完成后自动进入审核页面。"}
                p { @if cfg!(target_os="macos") {"当前能力：PNG 无损 · JPEG / HEIC · 透明度与画质检测"} @else {"当前能力：PNG 无损；JPEG / HEIC 编码仅在 macOS 可用。"} }
                p id="progress" role="status" aria-live="polite" {"正在扫描目录与 Git 忽略规则…"}
                p {"报告与恢复备份保留在：" (out.display())}
            }
            script {(PreEscaped(r#"async function poll(){try{const r=await fetch('/api/progress',{cache:'no-store'});if(!r.ok)throw Error('connection');const p=await r.json();if(p.done){location.reload();return;}if(p.error){document.getElementById('progress').textContent='分析失败：'+p.error;return;}document.getElementById('progress').textContent=p.total?'已分析 '+p.completed+' / '+p.total+' 个资源':'正在扫描目录与 Git 忽略规则…';setTimeout(poll,700);}catch{document.getElementById('progress').textContent='本地服务连接中断，请确认终端进程仍在运行。';setTimeout(poll,2000);}}poll();"#))}
            }
        }
    }.into_string()
}
