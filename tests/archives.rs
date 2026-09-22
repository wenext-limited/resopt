use resopt::{AnalysisOptions, analyze};
use std::{
    fs,
    io::{Cursor, Write},
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn fixture() -> Vec<u8> {
    let mut image = vec![];
    {
        let mut encoder = png::Encoder::new(&mut image, 32, 32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::NoCompression);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[128; 32 * 32 * 4])
            .unwrap();
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("assets/image.png", image.as_slice()),
        ("assets/image.atlas", b"image.png\nsize: 32,32\n".as_slice()),
        ("manifest.json", b"{}".as_slice()),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn report_browses_zip_with_previews_without_touching_source_or_extracting_paths() {
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let bytes = fixture();
    fs::write(root.path().join("game.zip"), &bytes).unwrap();
    let report = analyze(
        root.path(),
        output.path().join("report"),
        AnalysisOptions {
            probe_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    let row = &report.resources[0];
    assert_eq!(row.status, "inspected");
    let archive = row.archive.as_ref().unwrap();
    assert_eq!(archive.entries.len(), 3);
    assert!(
        output
            .path()
            .join("report")
            .join(archive.entries[0].preview.as_ref().unwrap())
            .is_file()
    );
    assert_eq!(archive.rewrite_blockers.len(), 1);
    assert!(row.candidates.is_empty());
    assert!(!root.path().join("assets").exists());
    assert_eq!(fs::read(root.path().join("game.zip")).unwrap(), bytes);
    let json = serde_json::to_vec(&report).unwrap();
    let restored: resopt::AnalysisReport = serde_json::from_slice(&json).unwrap();
    assert_eq!(
        restored.resources[0].archive.as_ref().unwrap().entries[0].path,
        "assets/image.png"
    );
}
