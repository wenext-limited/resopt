use resopt::refresh_report;
use serde_json::json;
use std::fs;

fn fixture(root: &std::path::Path, name: &str) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schema_version":1,"root":root,"backend":"test","options":{},
        "inventory":{"schema_version":2,"root":root,"catalogs":0,"assets":[],
            "skipped_source_or_tooling_files":0,"excluded_directories":[],"diagnostics":[]},
        "resources":[{"resource":{"path":name,"bytes":1536,"kind":"data","format":"json",
            "extension":"json","extension_mismatch":false,"origin":"loose_file","conversion_exclusion":null},
            "sha256":null,"image":null,"status":"inventory_only","issues":[],"candidates":[],
            "smallest_candidate":null,"original_preview":null,"original_artifact":null}],
        "status_counts":{"inventory_only":1},"potential_source_bytes_saved":0
    })).unwrap()
}

#[test]
fn refresh_changes_only_html_and_supports_cli() {
    let dir = tempfile::tempdir().unwrap();
    let json = fixture(dir.path(), "data.json");
    fs::write(dir.path().join("analysis.json"), &json).unwrap();
    fs::create_dir(dir.path().join("candidates")).unwrap();
    fs::write(
        dir.path().join("candidates/asset.png"),
        b"unchanged artifact",
    )
    .unwrap();
    fs::write(dir.path().join("report.html"), b"old presentation").unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_resopt"))
        .arg("report")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(dir.path().join("analysis.json")).unwrap(), json);
    assert_eq!(
        fs::read(dir.path().join("candidates/asset.png")).unwrap(),
        b"unchanged artifact"
    );
    let html = fs::read_to_string(dir.path().join("report.html")).unwrap();
    assert!(html.contains("id=\"search\""));
    assert!(html.contains("KiB"));
    assert!(!html.contains("__RESOPT_DATA__"));
}

#[test]
fn resource_text_cannot_terminate_the_embedded_json_script() {
    let dir = tempfile::tempdir().unwrap();
    let name = "evil</script><script>alert('x')</script>.json";
    fs::write(dir.path().join("analysis.json"), fixture(dir.path(), name)).unwrap();
    let output = refresh_report(dir.path()).unwrap();
    let html = fs::read_to_string(output).unwrap();
    assert!(!html.contains(name));
    assert!(html.contains("\\u003c/script\\u003e"));
    let payload = html
        .split("id=\"report-data\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let decoded: serde_json::Value = serde_json::from_str(payload).unwrap();
    assert_eq!(decoded["resources"][0]["resource"]["path"], name);
}

#[test]
fn unsupported_schema_does_not_replace_existing_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fixture(dir.path(), "data.json")).unwrap();
    value["schema_version"] = json!(99);
    fs::write(
        dir.path().join("analysis.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    fs::write(dir.path().join("report.html"), b"keep this report").unwrap();
    assert!(refresh_report(dir.path()).is_err());
    assert_eq!(
        fs::read(dir.path().join("report.html")).unwrap(),
        b"keep this report"
    );
}
