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
