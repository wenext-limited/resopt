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
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(stream,"{method} {route} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",body.len()).unwrap();
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
    head + &String::from_utf8(body).unwrap()
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
    let origin = line
        .trim()
        .strip_prefix("Review server: ")
        .unwrap()
        .trim_end_matches('/');
    let address = origin.strip_prefix("http://").unwrap();
    let page = request(address, "GET", "/", "", "");
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
    assert!(
        request(address, "GET", "/operations/0/transaction.json", "", "")
            .starts_with("HTTP/1.1 404")
    );
    assert!(request(address, "GET", "/../Cargo.toml", "", "").starts_with("HTTP/1.1 404"));
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
