//! Loopback-only application server for `resopt web` and `resopt serve`.
//!
//! The server binds 127.0.0.1, never accepts filesystem paths or file uploads
//! from HTTP clients, and checks the Host header on every request (DNS
//! rebinding), a per-process session token on every API route, and the Origin
//! header on every state-changing request (cross-site requests).
//!
//! The page itself is served only to the launch URL, which carries a one-time
//! key; it then sets an HttpOnly, SameSite=Strict cookie that report artifacts
//! require. Another local process that merely knows the port can therefore
//! neither obtain the session token nor read project data.
use crate::{
    AnalysisControl, ResourceAnalysis,
    batch::{self, BatchPlan, BatchPolicy, BatchStatus},
    filesystem::contained_file,
    review::{Approvals, Review},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY_BYTES: usize = 256 * 1024;
const RESULTS_PAGE: usize = 500;
const HTTP_WORKERS: usize = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    resource: usize,
    candidate: Option<usize>,
    #[serde(default)]
    approve_lossy: bool,
    /// Legacy flag: approves both Alpha warning kinds.
    #[serde(default)]
    approve_alpha_loss: bool,
    #[serde(default)]
    approve_warnings: Vec<String>,
    #[serde(default)]
    plan_token: Option<String>,
}

impl Action {
    fn warnings(&self) -> Vec<String> {
        let mut warnings = self.approve_warnings.clone();
        if self.approve_alpha_loss {
            for kind in [
                "alpha_error_exceeds_policy",
                "transparency_presence_changed",
            ] {
                if !warnings.iter().any(|w| w == kind) {
                    warnings.push(kind.to_string());
                }
            }
        }
        warnings
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchRequest {
    policy: BatchPolicy,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreRequest {
    resources: Vec<usize>,
}

enum BatchJob {
    Apply(BatchPlan),
    Restore(Option<Vec<usize>>),
}

/// Analysis progress shared between the worker thread and HTTP handlers.
#[derive(Default)]
pub(crate) struct Live {
    /// Completed rows in completion order, tagged with their report index.
    pub rows: Vec<(usize, ResourceAnalysis)>,
    pub total: usize,
    pub error: Option<String>,
    pub cancelled: bool,
}

#[derive(Serialize)]
struct ResultsPage<'a> {
    phase: &'static str,
    completed: usize,
    total: usize,
    candidates: usize,
    savings_bytes: u64,
    next: usize,
    rows: Vec<Row<'a>>,
    error: Option<&'a str>,
}
#[derive(Serialize)]
struct Row<'a> {
    index: usize,
    row: &'a ResourceAnalysis,
}

pub(crate) struct App {
    pub directory: PathBuf,
    pub project: PathBuf,
    pub live: Mutex<Live>,
    pub control: AnalysisControl,
    /// Set once analysis has finished and the report passed validation.
    pub review: OnceLock<Review>,
    batch_status: Mutex<BatchStatus>,
    batch_cancel: AtomicBool,
    batch_running: AtomicBool,
}

impl App {
    pub fn new(directory: PathBuf, project: PathBuf) -> Self {
        Self {
            directory,
            project,
            live: Mutex::new(Live::default()),
            control: AnalysisControl::default(),
            review: OnceLock::new(),
            batch_status: Mutex::new(BatchStatus::default()),
            batch_cancel: AtomicBool::new(false),
            batch_running: AtomicBool::new(false),
        }
    }

    /// Publish a finished report: its rows replace the live rows.
    pub fn finish(&self, review: Review) {
        {
            let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
            live.total = review.report.resources.len();
            live.cancelled = review.report.cancelled;
            live.rows = review
                .report
                .resources
                .iter()
                .cloned()
                .enumerate()
                .collect();
        }
        let _ = self.review.set(review);
    }

    fn review(&self) -> Result<&Review> {
        self.review
            .get()
            .context("analysis is still running; changes can be applied once it completes")
    }
}

/// Serve an existing analysis on loopback. Port 0 chooses an available port.
/// The printed URL is the entry point; terminate the process to stop serving.
pub fn serve(directory: impl AsRef<Path>, port: u16) -> Result<()> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow::anyhow!("{e}"))?;
    let review = Review::open(directory.as_ref())?;
    let app = Arc::new(App::new(
        review.directory.clone(),
        review.report.root.clone(),
    ));
    app.finish(review);
    let token = session_token()?;
    println!(
        "Review server: {}\nProject: {}\nStop with Ctrl-C. Sources change only after an explicit Apply request.",
        launch_url(&server, &token),
        app.project.display()
    );
    std::io::stdout().flush()?;
    run(Arc::new(server), app, token)
}

pub(crate) fn session_token() -> Result<String> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
    Ok(random.iter().map(|b| format!("{b:02x}")).collect())
}

/// The only URL that serves the page: it carries the session key.
pub(crate) fn launch_url(server: &Server, token: &str) -> String {
    format!("http://{}/?k={token}", server.server_addr())
}

/// Handle requests until the process exits.
pub(crate) fn run(server: Arc<Server>, app: Arc<App>, token: String) -> Result<()> {
    let address = server.server_addr().to_string();
    let page = crate::report::render_live_page(&app.project, &token)?;
    let session = Arc::new(Session {
        cookie: format!("resopt_{}", address.rsplit(':').next().unwrap_or_default()),
        origin: format!("http://{address}"),
        address,
        token,
        page,
    });
    let workers: Vec<_> = (0..HTTP_WORKERS)
        .map(|_| {
            let (server, app, session) = (server.clone(), app.clone(), session.clone());
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle(request, &app, &session);
                }
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

struct Session {
    /// Cookie names are not port-scoped, so each server uses its own.
    cookie: String,
    address: String,
    origin: String,
    token: String,
    page: String,
}

fn json(request: Request, code: u16, value: &impl Serialize) {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| br#"{"error":"response serialization failed"}"#.to_vec());
    respond(request, code, "application/json", body);
}

fn error(request: Request, code: u16, message: &str) {
    json(request, code, &serde_json::json!({ "error": message }));
}

fn handle(mut request: Request, app: &Arc<App>, session: &Session) {
    if header(&request, "Host") != Some(session.address.as_str()) {
        return error(request, 403, "invalid Host");
    }
    let url = request.url().to_string();
    let (route, query) = url.split_once('?').unwrap_or((&url, ""));
    let get = request.method() == &Method::Get;
    let post = request.method() == &Method::Post;
    if !get && !post {
        return error(
            request,
            405,
            "method not allowed; this server accepts no uploads",
        );
    }
    let has_cookie = header(&request, "Cookie").is_some_and(|cookies| {
        cookies
            .split(';')
            .filter_map(|pair| pair.trim().split_once('='))
            .any(|(name, value)| name == session.cookie && value == session.token)
    });
    if get && matches!(route, "/" | "/report.html") {
        let has_key = query
            .split('&')
            .any(|pair| pair.strip_prefix("k=") == Some(session.token.as_str()));
        if !has_key && !has_cookie {
            return respond(
                request,
                403,
                "text/plain; charset=utf-8",
                b"Open the full URL that resopt printed in your terminal (it contains the session key).".to_vec(),
            );
        }
        let cookie = format!(
            "{}={}; HttpOnly; SameSite=Strict; Path=/",
            session.cookie, session.token
        );
        return respond_with(
            request,
            200,
            "text/html; charset=utf-8",
            session.page.as_bytes().to_vec(),
            &[("Set-Cookie", cookie.as_str())],
        );
    }
    if get && route == "/favicon.ico" {
        return respond(request, 204, "image/x-icon", vec![]);
    }
    if get && !route.starts_with("/api/") {
        // Images cannot send custom headers; the session cookie authorizes them.
        if !has_cookie && header(&request, "X-Resopt-Token") != Some(session.token.as_str()) {
            return error(
                request,
                403,
                "invalid session; open the URL printed by resopt",
            );
        }
        return serve_artifact(request, app, route);
    }
    if !route.starts_with("/api/") {
        return error(request, 404, "not found");
    }
    if header(&request, "X-Resopt-Token") != Some(session.token.as_str()) {
        return error(
            request,
            403,
            "invalid session; reload the page opened by resopt",
        );
    }
    if post
        && (header(&request, "Origin") != Some(session.origin.as_str())
            || header(&request, "Content-Type") != Some("application/json"))
    {
        return error(request, 403, "invalid origin or content type");
    }
    let result = if get {
        api_get(app, route, query)
    } else {
        read_body(&mut request).and_then(|body| api_post(app, route, &body))
    };
    match result {
        Ok(Some(value)) => json(request, 200, &value),
        Ok(None) => error(request, 404, "not found"),
        Err(failure) => {
            let states = app.review.get().map(Review::states);
            json(
                request,
                409,
                &serde_json::json!({"error": format!("{failure:#}"), "states": states}),
            );
        }
    }
}

fn read_body(request: &mut Request) -> Result<Vec<u8>> {
    ensure!(
        request.body_length().is_some_and(|n| n <= MAX_BODY_BYTES),
        "request too large or missing content length"
    );
    let mut body = vec![];
    request
        .as_reader()
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_end(&mut body)?;
    ensure!(body.len() <= MAX_BODY_BYTES, "request too large");
    Ok(body)
}

fn api_get(app: &Arc<App>, route: &str, query: &str) -> Result<Option<serde_json::Value>> {
    Ok(Some(match route {
        "/api/capabilities" => serde_json::to_value(crate::capabilities())?,
        "/api/results" => {
            let after = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("after="))
                .map(|value| value.parse::<usize>())
                .transpose()
                .context("invalid after parameter")?
                .unwrap_or(0);
            let live = app.live.lock().unwrap_or_else(|e| e.into_inner());
            let rows: Vec<_> = live
                .rows
                .iter()
                .skip(after)
                .take(RESULTS_PAGE)
                .map(|(index, row)| Row { index: *index, row })
                .collect();
            serde_json::to_value(ResultsPage {
                phase: if live.error.is_some() {
                    "failed"
                } else if app.review.get().is_some() {
                    "ready"
                } else {
                    "analyzing"
                },
                completed: live.rows.len(),
                total: live.total,
                candidates: live
                    .rows
                    .iter()
                    .filter(|(_, r)| r.recommended_savings() > 0)
                    .count(),
                savings_bytes: live.rows.iter().map(|(_, r)| r.recommended_savings()).sum(),
                next: after + rows.len(),
                rows,
                error: live.error.as_deref(),
            })?
        }
        "/api/report" => crate::report::meta(&app.review()?.report),
        "/api/state" => match app.review.get() {
            Some(review) => review.states(),
            None => serde_json::json!({}),
        },
        "/api/batch" => {
            serde_json::to_value(&*app.batch_status.lock().unwrap_or_else(|e| e.into_inner()))?
        }
        _ => return Ok(None),
    }))
}

fn api_post(app: &Arc<App>, route: &str, body: &[u8]) -> Result<Option<serde_json::Value>> {
    match route {
        "/api/cancel" => {
            app.control.cancel();
            return Ok(Some(serde_json::json!({"ok": true})));
        }
        "/api/batch/cancel" => {
            app.batch_cancel.store(true, Ordering::SeqCst);
            return Ok(Some(serde_json::json!({"ok": true})));
        }
        _ => {}
    }
    let review = app.review()?;
    Ok(Some(match route {
        "/api/preview" => {
            let action: Action = serde_json::from_slice(body)?;
            review.preview_with_warnings(
                action.resource,
                action.candidate.context("missing candidate")?,
                &action.warnings(),
            )?
        }
        "/api/apply" | "/api/restore" => {
            ensure!(
                !app.batch_running.load(Ordering::SeqCst),
                "a batch is running; wait for it to finish or cancel it"
            );
            let action: Action = serde_json::from_slice(body)?;
            if route == "/api/apply" {
                review.apply_with_warnings(
                    action.resource,
                    action.candidate.context("missing candidate")?,
                    &Approvals {
                        lossy: action.approve_lossy,
                        warnings: action.warnings(),
                    },
                    action.plan_token.as_deref(),
                    true,
                )?;
            } else {
                review.restore(action.resource)?;
            }
            serde_json::json!({"ok": true, "states": review.states()})
        }
        "/api/batch/preview" => {
            let request: BatchRequest = serde_json::from_slice(body)?;
            serde_json::to_value(batch::plan(review, &request.policy)?)?
        }
        "/api/batch/apply" => {
            let request: BatchRequest = serde_json::from_slice(body)?;
            let plan = batch::plan(review, &request.policy)?;
            ensure!(
                request.token.as_deref() == Some(plan.token.as_str()),
                "the project or policy changed after the preview; review the batch again"
            );
            start_batch(app, BatchJob::Apply(plan))?;
            serde_json::json!({"ok": true})
        }
        "/api/batch/restore" => {
            let mut request: RestoreRequest = serde_json::from_slice(body)?;
            ensure!(
                !request.resources.is_empty(),
                "select at least one resource to restore"
            );
            request.resources.sort_unstable();
            request.resources.dedup();
            ensure!(
                request
                    .resources
                    .iter()
                    .all(|&index| index < review.report.resources.len()),
                "invalid resource index"
            );
            start_batch(app, BatchJob::Restore(Some(request.resources)))?;
            serde_json::json!({"ok": true})
        }
        "/api/restore-all" => {
            start_batch(app, BatchJob::Restore(None))?;
            serde_json::json!({"ok": true})
        }
        _ => return Ok(None),
    }))
}

/// Run an apply or restore batch on a background thread.
fn start_batch(app: &Arc<App>, job: BatchJob) -> Result<()> {
    ensure!(
        !app.batch_running.swap(true, Ordering::SeqCst),
        "another batch is already running"
    );
    app.batch_cancel.store(false, Ordering::SeqCst);
    // Reset before returning, so a client that polls right after its request is
    // accepted never reads the outcome of the previous batch.
    *app.batch_status.lock().unwrap_or_else(|e| e.into_inner()) = BatchStatus {
        running: true,
        total: match &job {
            BatchJob::Apply(plan) => plan.items.len(),
            BatchJob::Restore(Some(resources)) => resources.len(),
            BatchJob::Restore(None) => 0,
        },
        ..Default::default()
    };
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(review) = app.review.get() {
            match job {
                BatchJob::Apply(plan) => {
                    batch::run(review, &plan, &app.batch_status, &app.batch_cancel)
                }
                BatchJob::Restore(resources) => {
                    let status =
                        batch::restore_many(review, resources.as_deref(), &app.batch_cancel);
                    *app.batch_status.lock().unwrap_or_else(|e| e.into_inner()) = status;
                }
            }
        }
        app.batch_running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Report artifacts are addressed only as `<folder>/<generated-name>`.
fn artifact_path(route: &str) -> Option<PathBuf> {
    if route == "/analysis.json" {
        return Some(PathBuf::from("analysis.json"));
    }
    let (folder, name) = route.strip_prefix('/')?.split_once('/')?;
    let generated = !name.is_empty()
        && name.len() <= 96
        && name.as_bytes()[0].is_ascii_digit()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        && !name.contains("..");
    (matches!(folder, "previews" | "candidates" | "originals") && generated)
        .then(|| Path::new(folder).join(name))
}

fn serve_artifact(request: Request, app: &App, route: &str) {
    let Some(relative) = artifact_path(route) else {
        return respond(request, 404, "text/plain", b"Not found".to_vec());
    };
    let media = match relative.extension().and_then(|v| v.to_str()) {
        Some("png") => "image/png",
        Some("jpeg" | "jpg") => "image/jpeg",
        Some("heic") => "image/heic",
        Some("webp") => "image/webp",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    match contained_file(&app.directory, &relative).and_then(|p| crate::resources::bounded_read(&p))
    {
        Ok(bytes) => respond(request, 200, media, bytes),
        Err(_) => respond(request, 404, "text/plain", b"Artifact unavailable".to_vec()),
    }
}

pub(crate) fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.to_string().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

pub(crate) fn respond(request: Request, code: u16, media: &str, bytes: Vec<u8>) {
    respond_with(request, code, media, bytes, &[]);
}

fn respond_with(request: Request, code: u16, media: &str, bytes: Vec<u8>, extra: &[(&str, &str)]) {
    let mut response = Response::from_data(bytes).with_status_code(StatusCode(code));
    for (key, value) in [
        ("Content-Type", media),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        ("Cross-Origin-Resource-Policy", "same-origin"),
        (
            "Content-Security-Policy",
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    ]
    .into_iter()
    .chain(extra.iter().copied())
    {
        if let Ok(header) = Header::from_bytes(key, value) {
            response.add_header(header);
        }
    }
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_generated_artifact_names_are_served() {
        for route in [
            "/previews/12-webp-85.png",
            "/candidates/0-png-0.png",
            "/originals/7.heic",
            "/analysis.json",
        ] {
            assert!(artifact_path(route).is_some(), "{route}");
        }
        for route in [
            "/previews/../analysis.json",
            "/previews/..%2f..%2fetc",
            "/operations/0/transaction.json",
            "/previews/",
            "/previews/a/b.png",
            "/candidates/x.png",
            "/previews/1\\..\\x",
            "//etc/passwd",
            "/report.html/../x",
        ] {
            assert!(artifact_path(route).is_none(), "{route}");
        }
    }

    #[test]
    fn legacy_alpha_flag_maps_to_both_alpha_warnings() {
        let action: Action =
            serde_json::from_str(r#"{"resource":0,"candidate":1,"approve_alpha_loss":true}"#)
                .unwrap();
        assert_eq!(
            action.warnings(),
            [
                "alpha_error_exceeds_policy",
                "transparency_presence_changed"
            ]
        );
        assert!(serde_json::from_str::<Action>(r#"{"resource":0,"path":"/etc/passwd"}"#).is_err());
    }
}
