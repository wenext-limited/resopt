//! Local project companion: filesystem analysis and review, never an upload API.
use crate::{
    AnalysisOptions,
    analysis::analyze_with_observer,
    review::Review,
    server::{self, App},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};
use tiny_http::Server;

#[derive(Default)]
pub struct WebOptions {
    pub out: Option<PathBuf>,
    pub port: u16,
    pub no_open: bool,
    pub analysis: AnalysisOptions,
}
/// Start a local-only project analysis and serve the review UI while it runs.
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
    let token = server::session_token()?;
    let url = server::launch_url(&server, &token);
    println!(
        "Local web: {url}\nProject: {}\nReport: {}\nStop with Ctrl-C. Files stay on this device; reports and restore backups are retained.\nThe URL contains this session's key; the page is not served without it.",
        root.display(),
        out.display()
    );
    std::io::stdout().flush()?;
    let app = Arc::new(App::new(out.clone(), root.clone()));
    let worker_app = app.clone();
    let analysis = options.analysis;
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            analyze_with_observer(
                &root,
                &out,
                analysis,
                &worker_app.control,
                |index, resource, _, total| {
                    let mut live = worker_app.live.lock().unwrap_or_else(|e| e.into_inner());
                    live.total = total;
                    live.rows.push((
                        index,
                        crate::ResourceAnalysis {
                            fingerprint: None,
                            ..resource.clone()
                        },
                    ));
                },
            )
            .and_then(|_| Review::open(&out))
        }));
        match result {
            Ok(Ok(review)) => worker_app.finish(review),
            Ok(Err(error)) => {
                worker_app
                    .live
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .error = Some(format!("{error:#}"));
            }
            Err(_) => {
                worker_app
                    .live
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .error = Some(
                    "The analysis thread stopped unexpectedly. Check the terminal output and run resopt web again."
                        .into(),
                );
            }
        }
    });
    if !options.no_open {
        open_browser(&url);
    }
    server::run(Arc::new(server), app, token)
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
        Err(error) => eprintln!("Could not open browser ({error}); open {origin} manually."),
    }
}
