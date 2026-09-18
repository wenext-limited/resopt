//! Loopback-only review server. Never accepts filesystem paths from HTTP clients.
use crate::{filesystem::contained_file, report::render_page, review::Review};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    resource: usize,
    candidate: Option<usize>,
    #[serde(default)]
    approve_lossy: bool,
    #[serde(default)]
    plan_token: Option<String>,
}

/// Serve an existing analysis on loopback. Port 0 chooses an available port.
/// The printed URL is the entry point; terminate the process to stop serving.
pub fn serve(directory: impl AsRef<Path>, port: u16) -> Result<()> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow::anyhow!("{e}"))?;
    serve_on(directory.as_ref(), server)
}

pub(crate) fn serve_on(directory: &Path, server: Server) -> Result<()> {
    let review = Review::open(directory)?;
    let address = server.server_addr().to_string();
    let origin = format!("http://{address}");
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let html = render_page(&review.report, Some(&token))?;
    let mut assets = BTreeMap::<String, (PathBuf, String)>::new();
    assets.insert(
        "/analysis.json".into(),
        (PathBuf::from("analysis.json"), "application/json".into()),
    );
    for r in &review.report.resources {
        for path in [r.original_artifact.as_ref(), r.original_preview.as_ref()]
            .into_iter()
            .flatten()
            .chain(
                r.candidates
                    .iter()
                    .flat_map(|c| [c.artifact.as_ref(), c.preview.as_ref()])
                    .flatten(),
            )
        {
            let media = match path.extension().and_then(|v| v.to_str()) {
                Some("png") => "image/png",
                Some("jpeg" | "jpg") => "image/jpeg",
                Some("heic") => "image/heic",
                _ => "application/octet-stream",
            };
            let url = format!("/{}", path.to_string_lossy().replace('\\', "/"));
            ensure!(url.is_ascii(), "non-ASCII artifact path is unsupported");
            assets.insert(url, (path.clone(), media.into()));
        }
    }
    println!(
        "Review server: {origin}/\nProject: {}\nStop with Ctrl-C. Sources change only after an explicit Apply request.",
        review.report.root.display()
    );
    std::io::stdout().flush()?;
    for mut request in server.incoming_requests() {
        if header(&request, "Host") != Some(address.as_str()) {
            respond(
                request,
                403,
                "application/json",
                br#"{"error":"invalid Host"}"#.to_vec(),
            );
            continue;
        }
        let route = request.url().to_string();
        if request.method() == &Method::Get && matches!(route.as_str(), "/" | "/report.html") {
            respond(
                request,
                200,
                "text/html; charset=utf-8",
                html.as_bytes().to_vec(),
            );
        } else if request.method() == &Method::Get && route == "/api/progress" {
            respond(
                request,
                200,
                "application/json",
                br#"{"done":true}"#.to_vec(),
            );
        } else if request.method() == &Method::Get && route == "/api/state" {
            if header(&request, "X-Resopt-Token") != Some(token.as_str()) {
                respond(
                    request,
                    403,
                    "application/json",
                    br#"{"error":"invalid session"}"#.to_vec(),
                );
                continue;
            }
            respond(
                request,
                200,
                "application/json",
                serde_json::to_vec(&review.states())?,
            );
        } else if request.method() == &Method::Post
            && matches!(
                route.as_str(),
                "/api/apply" | "/api/restore" | "/api/preview"
            )
        {
            if header(&request, "Origin") != Some(origin.as_str())
                || header(&request, "X-Resopt-Token") != Some(token.as_str())
                || header(&request, "Content-Type") != Some("application/json")
            {
                respond(
                    request,
                    403,
                    "application/json",
                    br#"{"error":"invalid origin or session"}"#.to_vec(),
                );
                continue;
            }
            let result = (|| -> Result<serde_json::Value> {
                ensure!(
                    request.body_length().is_some_and(|n| n <= 4096),
                    "request too large or missing content length"
                );
                let mut body = vec![];
                request.as_reader().take(4097).read_to_end(&mut body)?;
                ensure!(body.len() <= 4096, "request too large");
                let action: Action = serde_json::from_slice(&body)?;
                if route == "/api/preview" {
                    return review.preview(
                        action.resource,
                        action.candidate.context("missing candidate")?,
                    );
                }
                if route == "/api/apply" {
                    review.apply_reviewed(
                        action.resource,
                        action.candidate.context("missing candidate")?,
                        action.approve_lossy,
                        action.plan_token.as_deref(),
                    )?;
                } else {
                    review.restore(action.resource)?;
                }
                Ok(serde_json::json!({"ok":true,"states":review.states()}))
            })();
            let (code, body) = match result {
                Ok(body) => (200, body),
                Err(e) => (
                    409,
                    serde_json::json!({"error":format!("{e:#}"),"states":review.states()}),
                ),
            };
            respond(
                request,
                code,
                "application/json",
                serde_json::to_vec(&body)?,
            );
        } else if request.method() == &Method::Get && assets.contains_key(&route) {
            let (path, media) = &assets[&route];
            match contained_file(&review.directory, path)
                .and_then(|p| crate::resources::bounded_read(&p))
            {
                Ok(bytes) => respond(request, 200, media, bytes),
                Err(_) => respond(request, 404, "text/plain", b"Artifact unavailable".to_vec()),
            }
        } else {
            respond(request, 404, "text/plain", b"Not found".to_vec());
        }
    }
    Ok(())
}

pub(crate) fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.to_string().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
pub(crate) fn respond(request: Request, code: u16, media: &str, bytes: Vec<u8>) {
    let mut response = Response::from_data(bytes).with_status_code(StatusCode(code));
    for (key, value) in [
        ("Content-Type", media),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        (
            "Content-Security-Policy",
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    ] {
        response.add_header(Header::from_bytes(key, value).expect("static valid header"));
    }
    let _ = request.respond(response);
}
