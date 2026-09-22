use resopt::{AnalysisOptions, analyze};
use std::fs;

#[test]
fn vap_is_detected_inside_mp4_and_staged_without_transcoding() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let config = br#"{"info":{"v":2,"w":64,"h":64,"f":10,"fps":20,"videoW":96,"videoH":64,"rgbFrame":[0,0,64,64],"aFrame":[64,0,32,32]}}"#;
    let mut bytes = vec![0, 0, 0, 16];
    bytes.extend(b"ftypisom0000");
    bytes.extend(((config.len() + 8) as u32).to_be_bytes());
    bytes.extend(b"vapc");
    bytes.extend(config);
    fs::write(root.path().join("effect.mp4"), &bytes).unwrap();
    let report = analyze(
        root.path(),
        out.path().join("report"),
        AnalysisOptions {
            probe_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    let row = &report.resources[0];
    assert_eq!(row.resource.kind, "animation");
    assert_eq!(row.resource.format, "vap");
    assert_eq!(row.vap.as_ref().unwrap().frames, 10);
    assert!(row.candidates.is_empty());
    assert_eq!(
        fs::read(
            out.path()
                .join("report")
                .join(row.original_artifact.as_ref().unwrap())
        )
        .unwrap(),
        bytes
    );
    assert_eq!(fs::read(root.path().join("effect.mp4")).unwrap(), bytes);
}

#[test]
fn pag_signature_overrides_tcmp4_suffix_and_bundles_an_offline_sandbox() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let bytes = b"PAG\x01\x02\x00\x00\x00U\x00\x00";
    fs::write(root.path().join("renamed.tcmp4"), bytes).unwrap();
    assert_eq!(
        resopt::inventory(root.path()).unwrap().assets[0].format,
        "pag"
    );
    let report = analyze(
        root.path(),
        out.path().join("report"),
        AnalysisOptions {
            probe_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    let row = &report.resources[0];
    assert_eq!(row.resource.format, "pag");
    assert_eq!(row.resource.extension, "tcmp4");
    assert_eq!(row.pag.as_ref().unwrap().runtime_version, "4.3.51");
    assert!(row.candidates.is_empty());
    let directory = out.path().join("report");
    assert_eq!(
        fs::read(directory.join(row.original_artifact.as_ref().unwrap())).unwrap(),
        bytes
    );
    let frame = fs::read_to_string(directory.join("previews/0-pag-player.html")).unwrap();
    assert!(frame.contains("connect-src 'none'"));
    assert!(!frame.contains("/*__LIBPAG__*/"));
    assert!(directory.join("previews/0-libpag-4.3.51.wasm").is_file());
    assert!(directory.join("previews/0-libpag-license.txt").is_file());
}
