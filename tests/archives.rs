use resopt::{AnalysisOptions, analyze};
use std::{
    fs,
    io::{Cursor, Write},
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn fixture_with_metadata(name: &str, data: &[u8]) -> Vec<u8> {
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
        (name, data),
    ] {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn report_browses_zip_with_previews_without_touching_source_or_extracting_paths() {
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let bytes = fixture_with_metadata("manifest.json", b"{}");
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

#[test]
fn lossless_zip_candidates_apply_and_restore_as_one_package() {
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let bytes = fixture_with_metadata("config.json", br#"{"images":["assets/image.png"]}"#);
    fs::write(root.path().join("game.zip"), &bytes).unwrap();
    let directory = output.path().join("report");
    let report = analyze(
        root.path(),
        &directory,
        AnalysisOptions {
            min_savings_bytes: 1,
            cache_dir: None,
            ..Default::default()
        },
    )
    .unwrap();
    let row = &report.resources[0];
    assert_eq!(row.status, "candidates_available", "{:?}", row.issues);
    assert_eq!(row.archive.as_ref().unwrap().optimized_images, 1);
    assert!(!row.candidates[0].lossy);
    assert!(row.candidates[0].bytes < bytes.len() as u64);
    let status = resopt::apply_report(&directory, &resopt::BatchPolicy::default()).unwrap();
    assert_eq!((status.applied, status.failed), (1, 0), "{status:?}");
    let applied = fs::read(root.path().join("game.zip")).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(&applied)).unwrap();
    assert_eq!(zip.len(), 3);
    let mut atlas = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("assets/image.atlas").unwrap(), &mut atlas)
        .unwrap();
    assert_eq!(atlas, "image.png\nsize: 32,32\n");
    assert_eq!(resopt::restore_report(&directory).unwrap().applied, 1);
    assert_eq!(fs::read(root.path().join("game.zip")).unwrap(), bytes);
}

#[test]
fn digest_metadata_blocks_rewriting_even_under_an_unrecognized_filename() {
    for (name, data) in [
        ("manifest.json", b"{}".as_slice()),
        (
            "config.json",
            br#"{"md5AssetsMap":{"image":"abc"}}"#.as_slice(),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("game.zip"),
            fixture_with_metadata(name, data),
        )
        .unwrap();
        let report = analyze(
            root.path(),
            output.path().join("report"),
            AnalysisOptions {
                min_savings_bytes: 1,
                cache_dir: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(report.resources[0].candidates.is_empty());
        assert!(
            !report.resources[0]
                .archive
                .as_ref()
                .unwrap()
                .rewrite_blockers
                .is_empty()
        );
    }
}
