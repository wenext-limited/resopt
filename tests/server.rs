use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, ChildStdout, Command, Stdio},
    time::Duration,
};

struct Server {
    child: Child,
    // Keep the pipe open until the child exits. Dropping a temporary reader
    // after the first line can interrupt the rest of the banner on Windows.
    stdout: BufReader<ChildStdout>,
}
impl Server {
    fn new(mut child: Child) -> Self {
        let stdout = BufReader::new(child.stdout.take().expect("piped server stdout"));
        Self { child, stdout }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Fields, including stdout, are dropped only after the child is reaped.
    }
}

fn request(address: &str, method: &str, route: &str, headers: &str, body: &str) -> String {
    request_with_host(address, address, method, route, headers, body)
}

fn request_with_host(
    host: &str,
    address: &str,
    method: &str,
    route: &str,
    headers: &str,
    body: &str,
) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(stream,"{method} {route} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",body.len()).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let split = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let head = String::from_utf8(bytes[..split].to_vec()).unwrap();
    let mut body = bytes[split..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut decoded = Vec::new();
        let mut offset = 0;
        loop {
            let end = body[offset..]
                .windows(2)
                .position(|w| w == b"\r\n")
                .unwrap()
                + offset;
            let count = usize::from_str_radix(std::str::from_utf8(&body[offset..end]).unwrap(), 16)
                .unwrap();
            if count == 0 {
                break;
            }
            offset = end + 2;
            decoded.extend_from_slice(&body[offset..offset + count]);
            offset += count + 2;
        }
        body = decoded;
    }
    head + &String::from_utf8_lossy(&body)
}

#[test]
fn loopback_api_requires_session_and_origin_and_applies_only_selected_candidate() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("project");
    let asset = root.join("Assets.xcassets/Test.imageset");
    fs::create_dir_all(&asset).unwrap();
    fs::write(asset.join("Contents.json"),br#"{"images":[{"filename":"image.png","idiom":"universal"}],"info":{"version":1,"author":"xcode"}}"#).unwrap();
    let mut original = vec![];
    {
        let mut encoder = png::Encoder::new(&mut original, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::NoCompression);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[64, 128, 96, 255].repeat(4096))
            .unwrap();
    }
    fs::write(asset.join("image.png"), &original).unwrap();
    let plan_dir = base.path().join("plan");
    let plan = resopt::create_plan(
        &root,
        &plan_dir,
        resopt::Policy {
            min_input_bytes: 0,
            min_savings_bytes: 1,
            min_savings_percent: 0.0,
            ..Default::default()
        },
    )
    .unwrap();
    let candidate = &plan.candidates[0];
    let report_dir = base.path().join("report");
    fs::create_dir_all(report_dir.join("candidates")).unwrap();
    let optimized = fs::read(
        plan_dir
            .join("candidates")
            .join(format!("{}.png", candidate.optimized_sha256)),
    )
    .unwrap();
    fs::write(report_dir.join("candidates/0.png"), &optimized).unwrap();
    let report = json!({"schema_version":1,"root":plan.root,"backend":"test","options":{},"inventory":{"schema_version":2,"root":plan.root,"catalogs":1,"assets":[],"skipped_source_or_tooling_files":0,"excluded_directories":[],"diagnostics":[]},"resources":[{"resource":{"path":candidate.path,"bytes":candidate.original_bytes,"kind":"image","format":"png","extension":"png","extension_mismatch":false,"origin":"catalog_rendition","conversion_exclusion":null},"sha256":candidate.original_sha256,"image":null,"status":"candidates_available","issues":[],"candidates":[{"format":"png","quality":null,"lossy":false,"bytes":candidate.optimized_bytes,"savings_bytes":candidate.original_bytes-candidate.optimized_bytes,"valid":true,"rejection":null,"difference":null,"artifact":"candidates/0.png","preview":null}],"smallest_candidate":0,"original_preview":null,"original_artifact":null}],"status_counts":{"candidates_available":1},"potential_source_bytes_saved":candidate.original_bytes-candidate.optimized_bytes});
    fs::write(
        report_dir.join("analysis.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let mut server = Server::new(
        Command::new(env!("CARGO_BIN_EXE_resopt"))
            .args(["serve", report_dir.to_str().unwrap(), "--port", "0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    server.stdout.read_line(&mut line).unwrap();
    let mut project_line = String::new();
    server.stdout.read_line(&mut project_line).unwrap();
    assert!(project_line.starts_with("Project: "), "{project_line}");
    let mut ready_line = String::new();
    server.stdout.read_line(&mut ready_line).unwrap();
    assert!(ready_line.starts_with("Stop with Ctrl-C."), "{ready_line}");
    let origin = line.trim().strip_prefix("Review server: ").unwrap();
    let (origin, key) = origin
        .split_once("/?k=")
        .expect("launch URL carries the session key");
    let entry = format!("/?k={key}");
    let address = origin.strip_prefix("http://").unwrap();
    // Without the key (or the cookie it sets) neither the page nor data is served.
    assert!(request(address, "GET", "/", "", "").starts_with("HTTP/1.1 403"));
    assert!(request(address, "GET", "/?k=wrong", "", "").starts_with("HTTP/1.1 403"));
    assert!(request(address, "GET", "/analysis.json", "", "").starts_with("HTTP/1.1 403"));
    let page = request(address, "GET", &entry, "", "");
    assert!(page.to_ascii_lowercase().contains("set-cookie: resopt_"));
    assert!(page.contains("HttpOnly; SameSite=Strict"));
    assert!(page.starts_with("HTTP/1.1 200"));
    let data = page
        .split("id=\"report-data\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(data).unwrap();
    let token = data["sessionToken"].as_str().unwrap();
    let headers = format!(
        "Content-Type: application/json\r\nOrigin: {origin}\r\nX-Resopt-Token: {token}\r\n"
    );
    let body = r#"{"resource":0,"candidate":0,"approve_lossy":false}"#;
    assert!(
        request(
            address,
            "POST",
            "/api/apply",
            "Content-Type: application/json\r\n",
            body
        )
        .starts_with("HTTP/1.1 403")
    );
    assert!(
        request(
            address,
            "POST",
            "/api/apply",
            &headers.replace(origin, "https://untrusted.example"),
            body
        )
        .starts_with("HTTP/1.1 403")
    );
    assert!(request(address, "GET", "/api/state", "", "").starts_with("HTTP/1.1 403"));
    let cookie = format!(
        "Cookie: resopt_{}={token}\r\n",
        address.rsplit(':').next().unwrap()
    );
    assert!(
        request(
            address,
            "GET",
            "/operations/0/transaction.json",
            &cookie,
            ""
        )
        .starts_with("HTTP/1.1 404")
    );
    assert!(request(address, "GET", "/../Cargo.toml", &cookie, "").starts_with("HTTP/1.1 404"));
    assert!(request(address, "GET", "/analysis.json", &cookie, "").starts_with("HTTP/1.1 200"));
    assert_eq!(fs::read(asset.join("image.png")).unwrap(), original);
    assert!(
        request(
            address,
            "POST",
            "/api/preview",
            "Content-Type: application/json\r\n",
            body
        )
        .starts_with("HTTP/1.1 403")
    );
    let preview = request(address, "POST", "/api/preview", &headers, body);
    assert!(preview.starts_with("HTTP/1.1 200"), "{preview}");
    let preview: serde_json::Value =
        serde_json::from_str(preview.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(fs::read(asset.join("image.png")).unwrap(), original);
    let reviewed = json!({"resource":0,"candidate":0,"approve_lossy":false,"plan_token":preview["plan_token"]}).to_string();
    let result = request(address, "POST", "/api/apply", &headers, &reviewed);
    assert!(result.starts_with("HTTP/1.1 200"), "{result}");
    assert_eq!(fs::read(asset.join("image.png")).unwrap(), optimized);
    let result = request(
        address,
        "POST",
        "/api/restore",
        &headers,
        r#"{"resource":0}"#,
    );
    assert!(result.starts_with("HTTP/1.1 200"), "{result}");
    assert_eq!(fs::read(asset.join("image.png")).unwrap(), original);
}

#[test]
fn web_analyzes_local_project_without_uploads_and_serves_review() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "ignored.png\n").unwrap();
    let mut original = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut original, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::NoCompression);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[40, 80, 120, 128].repeat(64 * 64))
            .unwrap();
    }
    fs::write(root.join("image.png"), &original).unwrap();
    fs::write(root.join("ignored.png"), &original).unwrap();
    let out = base.path().join("analysis");
    let mut server = Server::new(
        Command::new(env!("CARGO_BIN_EXE_resopt"))
            .args([
                "web",
                root.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--no-open",
                "--no-cache",
                "--qualities",
                "85",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    server.stdout.read_line(&mut line).unwrap();
    let origin = line.trim().strip_prefix("Local web: ").unwrap().to_string();
    let (origin, key) = origin
        .split_once("/?k=")
        .expect("launch URL carries the session key");
    let (origin, entry) = (origin.to_string(), format!("/?k={key}"));
    let address = origin.strip_prefix("http://").unwrap();
    assert!(address.starts_with("127.0.0.1:"));
    // The shell page carries the session token and no project data.
    let page = request(address, "GET", &entry, "", "");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    let data = page
        .split("id=\"report-data\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(data).unwrap();
    assert!(data.get("resources").is_none());
    let token = data["sessionToken"].as_str().unwrap().to_string();
    let session = format!("X-Resopt-Token: {token}\r\n");
    let post = format!("Content-Type: application/json\r\nOrigin: {origin}\r\n{session}");
    let body_of = |response: &str| -> serde_json::Value {
        serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap()
    };

    // Every API route needs the session token, a loopback Host, and for
    // state changes the page's own Origin.
    assert!(request(address, "GET", "/api/results", "", "").starts_with("HTTP/1.1 403"));
    assert!(request(address, "GET", "/api/capabilities", "", "").starts_with("HTTP/1.1 403"));
    let rebinding = request_with_host("attacker.example", address, "GET", &entry, "", "");
    assert!(rebinding.starts_with("HTTP/1.1 403"), "{rebinding}");
    let cross_site = post.replace(&origin, "https://attacker.example");
    for route in ["/api/cancel", "/api/batch/apply", "/api/restore-all"] {
        let response = request(address, "POST", route, &cross_site, "{}");
        assert!(response.starts_with("HTTP/1.1 403"), "{route}: {response}");
        let no_token = request(
            address,
            "POST",
            route,
            "Content-Type: application/json\r\n",
            "{}",
        );
        assert!(no_token.starts_with("HTTP/1.1 403"), "{route}: {no_token}");
    }

    // Rows stream in while analysis runs; wait for the verified report.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let results = loop {
        let response = request(address, "GET", "/api/results?after=0", &session, "");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let results = body_of(&response);
        assert!(results["error"].is_null(), "{results}");
        if results["phase"] == "ready" {
            break results;
        }
        assert!(std::time::Instant::now() < deadline, "analysis timed out");
        std::thread::sleep(Duration::from_millis(50));
    };
    let rows = results["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{results}");
    assert_eq!(rows[0]["row"]["resource"]["path"], "image.png");
    let candidates = rows[0]["row"]["candidates"].as_array().unwrap();
    assert!(
        candidates
            .iter()
            .any(|c| c["format"] == "png" && c["artifact"].is_string() && c["sha256"].is_string())
    );
    #[cfg(not(target_os = "macos"))]
    assert!(candidates.iter().all(|c| c["format"] == "png"));
    assert_eq!(fs::read(root.join("image.png")).unwrap(), original);
    let capabilities = body_of(&request(address, "GET", "/api/capabilities", &session, ""));
    assert_eq!(
        capabilities["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == "heic")
            .unwrap()["available"],
        cfg!(target_os = "macos")
    );

    // No upload surface and no path traversal.
    let upload = request(
        address,
        "POST",
        "/api/convert",
        &format!("Content-Type: application/octet-stream\r\nOrigin: {origin}\r\n{session}"),
        "file bytes",
    );
    assert!(upload.starts_with("HTTP/1.1 403"), "{upload}");
    assert!(
        request(address, "PUT", "/previews/0-original.png", &session, "x")
            .starts_with("HTTP/1.1 405")
    );
    let cookie = format!(
        "Cookie: other=1; resopt_{}={token}\r\n",
        address.rsplit(':').next().unwrap()
    );
    assert!(
        request(address, "GET", "/../project/image.png", &cookie, "").starts_with("HTTP/1.1 404")
    );
    let artifact = candidates.iter().find(|c| c["format"] == "png").unwrap()["artifact"]
        .as_str()
        .unwrap();
    // Artifacts need the session cookie: knowing the port is not enough.
    assert!(request(address, "GET", &format!("/{artifact}"), "", "").starts_with("HTTP/1.1 403"));
    let stolen = format!(
        "Cookie: resopt_{}=guess\r\n",
        address.rsplit(':').next().unwrap()
    );
    assert!(
        request(address, "GET", &format!("/{artifact}"), &stolen, "").starts_with("HTTP/1.1 403")
    );
    assert!(
        request(address, "GET", &format!("/{artifact}"), &cookie, "").starts_with("HTTP/1.1 200")
    );

    // Batch: preview with the default lossless policy, confirm, then restore all.
    let policy = r#"{"policy":{}}"#;
    let plan = body_of(&request(
        address,
        "POST",
        "/api/batch/preview",
        &post,
        policy,
    ));
    assert_eq!(plan["items"].as_array().unwrap().len(), 1, "{plan}");
    assert_eq!(plan["lossy_items"], 0);
    let stale = request(
        address,
        "POST",
        "/api/batch/apply",
        &post,
        r#"{"policy":{},"token":"stale"}"#,
    );
    assert!(stale.starts_with("HTTP/1.1 409"), "{stale}");
    assert_eq!(fs::read(root.join("image.png")).unwrap(), original);
    let confirmed = json!({"policy": {}, "token": plan["token"]}).to_string();
    let started = request(address, "POST", "/api/batch/apply", &post, &confirmed);
    assert!(started.starts_with("HTTP/1.1 200"), "{started}");
    let wait_for_batch = || loop {
        let status = body_of(&request(address, "GET", "/api/batch", &session, ""));
        if status["running"] == false {
            break status;
        }
        assert!(std::time::Instant::now() < deadline, "batch timed out");
        std::thread::sleep(Duration::from_millis(30));
    };
    let status = wait_for_batch();
    assert_eq!(status["applied"], 1, "{status}");
    assert_eq!(status["outcomes"][0]["outcome"], "applied");
    assert!(fs::metadata(root.join("image.png")).unwrap().len() < original.len() as u64);
    let restoring = request(address, "POST", "/api/restore-all", &post, "{}");
    assert!(restoring.starts_with("HTTP/1.1 200"), "{restoring}");
    let status = wait_for_batch();
    assert_eq!(status["applied"], 1, "{status}");
    assert_eq!(fs::read(root.join("image.png")).unwrap(), original);
    let report = out.join("analysis.json");
    drop(server);
    assert!(report.is_file());
}

#[test]
fn web_rejects_existing_or_in_project_output_before_startup() {
    let root = tempfile::tempdir().unwrap();
    for out in [root.path().to_path_buf(), root.path().join("analysis")] {
        let result = Command::new(env!("CARGO_BIN_EXE_resopt"))
            .args([
                "web",
                root.path().to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--no-open",
            ])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn analysis_can_be_cancelled_and_still_yields_a_consistent_reviewable_report() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("project");
    fs::create_dir(&root).unwrap();
    for i in 0..48_u8 {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 256, 256);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::NoCompression);
        let data: Vec<u8> = (0..256 * 256_u32)
            .flat_map(|p| [(p % 251) as u8, (p / 256) as u8, i, 255])
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
        fs::write(root.join(format!("image-{i}.png")), bytes).unwrap();
    }
    let out = base.path().join("analysis");
    let mut server = Server::new(
        Command::new(env!("CARGO_BIN_EXE_resopt"))
            .args([
                "web",
                root.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--no-open",
                "--no-cache",
                "--jobs",
                "1",
                "--png-level",
                "4",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    server.stdout.read_line(&mut line).unwrap();
    let origin = line.trim().strip_prefix("Local web: ").unwrap().to_string();
    let (origin, key) = origin
        .split_once("/?k=")
        .expect("launch URL carries the session key");
    let (origin, entry) = (origin.to_string(), format!("/?k={key}"));
    let address = origin.strip_prefix("http://").unwrap();
    let page = request(address, "GET", &entry, "", "");
    let token = page
        .split("\"sessionToken\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let session = format!("X-Resopt-Token: {token}\r\n");
    let post = format!("Content-Type: application/json\r\nOrigin: {origin}\r\n{session}");
    // Changes are refused while analysis is running.
    let early = request(
        address,
        "POST",
        "/api/batch/preview",
        &post,
        r#"{"policy":{}}"#,
    );
    assert!(early.starts_with("HTTP/1.1 409"), "{early}");
    let cancelled = request(address, "POST", "/api/cancel", &post, "{}");
    assert!(cancelled.starts_with("HTTP/1.1 200"), "{cancelled}");
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        let response = request(address, "GET", "/api/results?after=100000", &session, "");
        if response.contains("\"phase\":\"ready\"") {
            break;
        }
        assert!(!response.contains("\"phase\":\"failed\""), "{response}");
        assert!(std::time::Instant::now() < deadline, "cancel timed out");
        std::thread::sleep(Duration::from_millis(50));
    }
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(report["cancelled"], true);
    let rows = report["resources"].as_array().unwrap();
    assert_eq!(rows.len(), 48);
    let skipped = rows
        .iter()
        .filter(|r| r["status"] == "not_analyzed")
        .count();
    assert!(
        skipped > 0,
        "cancelling early should leave unfinished files"
    );
    for row in rows {
        let status = row["status"].as_str().unwrap();
        assert!(
            matches!(
                status,
                "not_analyzed" | "candidates_available" | "inspected"
            ),
            "{status}"
        );
        if status == "not_analyzed" {
            assert!(row["candidates"].as_array().unwrap().is_empty());
        }
    }
    // Finished rows stay reviewable after a cancelled run.
    let plan = request(
        address,
        "POST",
        "/api/batch/preview",
        &post,
        r#"{"policy":{}}"#,
    );
    assert!(plan.starts_with("HTTP/1.1 200"), "{plan}");
}
