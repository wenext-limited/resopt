//! Cache reuse and invalidation, duplicate sharing, cancellation-safe reports
//! and report-level batch application through the public API.
use resopt::{AnalysisOptions, BatchPolicy, analyze, apply_report, plan_report, restore_report};
use std::{fs, path::Path};

fn png(seed: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_compression(png::Compression::NoCompression);
    let mut data = Vec::new();
    for y in 0..64_u32 {
        for x in 0..64_u32 {
            data.extend([(x * 3) as u8, (y * 3) as u8, seed, 255]);
        }
    }
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&data)
        .unwrap();
    bytes
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn options(cache: &Path) -> AnalysisOptions {
    AnalysisOptions {
        qualities: vec![85],
        jobs: 2,
        cache_dir: Some(cache.to_path_buf()),
        ..Default::default()
    }
}

#[test]
fn cache_hits_reproduce_results_and_invalidate_on_content_and_options() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    let cache = base.path().join("cache");
    write(&project, "a.png", &png(1));
    write(&project, "nested/b.png", &png(2));
    let first = analyze(&project, base.path().join("r1"), options(&cache)).unwrap();
    let perf = first.performance.as_ref().unwrap();
    assert_eq!(perf.cache_hits, 0);
    assert!(perf.phase_seconds.contains_key("decode"));
    assert!(perf.phase_seconds.contains_key("encode_lossless"));
    assert!(perf.first_result_seconds.is_some());

    let second = analyze(&project, base.path().join("r2"), options(&cache)).unwrap();
    assert_eq!(second.performance.as_ref().unwrap().cache_hits, 2);
    for (a, b) in first.resources.iter().zip(&second.resources) {
        assert_eq!(a.status, b.status);
        assert_eq!(a.smallest_candidate, b.smallest_candidate);
        assert_eq!(a.candidates.len(), b.candidates.len());
        for (x, y) in a.candidates.iter().zip(&b.candidates) {
            assert_eq!((x.bytes, &x.sha256, x.valid), (y.bytes, &y.sha256, y.valid));
            if let Some(artifact) = &y.artifact {
                // Restored artifacts are real files with the recorded content.
                let restored = fs::read(base.path().join("r2").join(artifact)).unwrap();
                assert_eq!(restored.len() as u64, y.bytes);
            }
        }
        assert!(
            b.original_artifact
                .as_ref()
                .is_none_or(|p| base.path().join("r2").join(p).is_file())
        );
    }

    // Changed content misses; the untouched file still hits.
    write(&project, "a.png", &png(9));
    let third = analyze(&project, base.path().join("r3"), options(&cache)).unwrap();
    assert_eq!(third.performance.as_ref().unwrap().cache_hits, 1);
    // Changed options miss entirely.
    let changed = AnalysisOptions {
        png_reductions: true,
        ..options(&cache)
    };
    let fourth = analyze(&project, base.path().join("r4"), changed).unwrap();
    assert_eq!(fourth.performance.as_ref().unwrap().cache_hits, 0);
}

#[test]
fn a_corrupted_cache_is_ignored_and_results_are_recomputed() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    let cache = base.path().join("cache");
    write(&project, "a.png", &png(3));
    let first = analyze(&project, base.path().join("r1"), options(&cache)).unwrap();
    let mut damaged = 0;
    for entry in walk(&cache) {
        if entry.file_name().is_some_and(|n| n != "entry.json") {
            fs::write(&entry, b"bit rot").unwrap();
            damaged += 1;
        }
    }
    assert!(damaged > 0);
    let second = analyze(&project, base.path().join("r2"), options(&cache)).unwrap();
    assert_eq!(second.performance.as_ref().unwrap().cache_hits, 0);
    assert_eq!(
        first.resources[0].candidates[0].sha256,
        second.resources[0].candidates[0].sha256
    );
    // The recomputed entry replaced the damaged one.
    let third = analyze(&project, base.path().join("r3"), options(&cache)).unwrap();
    assert_eq!(third.performance.as_ref().unwrap().cache_hits, 1);
}

fn walk(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = vec![];
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[test]
fn identical_files_are_analyzed_once_but_different_policies_are_not_shared() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    let bytes = png(5);
    for path in ["one.png", "two.png", "deep/three.png"] {
        write(&project, path, &bytes);
    }
    // Same bytes under Android policy must not reuse the loose-file result.
    write(&project, "app/src/main/res/drawable/four.png", &bytes);
    let report = analyze(
        &project,
        base.path().join("report"),
        AnalysisOptions {
            qualities: vec![85],
            webp: true,
            android_min_sdk: Some(21),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.performance.as_ref().unwrap().duplicate_reuses, 2);
    let formats = |suffix: &str| -> Vec<String> {
        let mut formats: Vec<_> = report
            .resources
            .iter()
            .find(|r| r.resource.path.ends_with(suffix))
            .unwrap()
            .candidates
            .iter()
            .map(|c| c.format.clone())
            .collect();
        formats.dedup();
        formats
    };
    assert_eq!(formats("one.png"), formats("deep/three.png"));
    assert!(
        !formats("four.png")
            .iter()
            .any(|f| f == "heic" || f == "jpeg")
    );
    for resource in &report.resources {
        assert_eq!(
            resource.resource.path.file_name(),
            resource.resource.path.file_name()
        );
        assert!(resource.sha256.is_some());
    }
}

#[test]
fn report_batch_applies_lossless_by_default_reports_conflicts_and_restores_all() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    let originals: Vec<_> = (0..4).map(|i| png(10 + i)).collect();
    for (i, bytes) in originals.iter().enumerate() {
        write(&project, &format!("img/{i}.png"), bytes);
    }
    let out = base.path().join("report");
    analyze(
        &project,
        &out,
        AnalysisOptions {
            qualities: vec![85],
            ..Default::default()
        },
    )
    .unwrap();

    // Invalid policies are refused before anything is touched.
    for policy in [
        BatchPolicy {
            lossless: false,
            ..Default::default()
        },
        BatchPolicy {
            accept_warnings: vec!["quality_below_policy".into()],
            ..Default::default()
        },
        BatchPolicy {
            lossy: true,
            accept_warnings: vec!["dimensions_changed".into()],
            ..Default::default()
        },
        BatchPolicy {
            min_score: Some(140.0),
            ..Default::default()
        },
    ] {
        assert!(plan_report(&out, &policy).is_err(), "{policy:?}");
    }
    let plan = plan_report(&out, &BatchPolicy::default()).unwrap();
    assert_eq!(plan.items.len(), 4);
    assert!(
        plan.items
            .iter()
            .all(|i| !i.lossy && i.format == "png" && i.warning.is_none())
    );
    assert!(
        plan.items
            .windows(2)
            .all(|w| w[0].savings_bytes >= w[1].savings_bytes)
    );

    // A file edited after analysis fails on its own; the others are applied.
    write(&project, "img/2.png", b"edited after analysis");
    let status = apply_report(&out, &BatchPolicy::default()).unwrap();
    assert_eq!((status.applied, status.failed), (3, 1));
    let failure = status
        .outcomes
        .iter()
        .find(|o| o.outcome == "failed")
        .unwrap();
    assert!(failure.path.ends_with("2.png"));
    assert!(failure.error.as_ref().unwrap().contains("hash mismatch"));
    assert_eq!(
        fs::read(project.join("img/2.png")).unwrap(),
        b"edited after analysis"
    );
    assert!(fs::metadata(project.join("img/0.png")).unwrap().len() < originals[0].len() as u64);

    // Applied files are skipped by a second batch rather than applied twice.
    assert_eq!(
        plan_report(&out, &BatchPolicy::default())
            .unwrap()
            .items
            .len(),
        1
    );

    // A later manual edit is never overwritten by restore-all.
    write(&project, "img/3.png", b"edited after apply");
    let restored = restore_report(&out).unwrap();
    assert_eq!((restored.applied, restored.failed), (2, 1));
    assert_eq!(fs::read(project.join("img/0.png")).unwrap(), originals[0]);
    assert_eq!(fs::read(project.join("img/1.png")).unwrap(), originals[1]);
    assert_eq!(
        fs::read(project.join("img/3.png")).unwrap(),
        b"edited after apply"
    );
}

/// Regression: nested rayon work inside oxipng used to re-enter the analyzer on
/// the same stack while a pixel lease or a duplicate slot was held, which hung
/// analysis forever. Duplicates, a tight pixel budget and a high PNG effort
/// made that likely; the run must now always finish.
#[test]
fn duplicates_under_a_tight_pixel_budget_never_deadlock() {
    fn large(seed: u8, side: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::Fast);
        let data: Vec<u8> = (0..side * side)
            .flat_map(|p| [(p % 251) as u8, (p / side) as u8, seed, 255])
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
        bytes
    }
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    for group in 0..6_u8 {
        let bytes = large(group, if group < 2 { 640 } else { 96 });
        for copy in 0..4 {
            write(&project, &format!("g{group}/copy-{copy}.png"), &bytes);
        }
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    for round in 0..3 {
        let (project, out, sender) = (
            project.clone(),
            base.path().join(format!("report-{round}")),
            sender.clone(),
        );
        std::thread::spawn(move || {
            let report = analyze(
                &project,
                out,
                AnalysisOptions {
                    qualities: vec![85],
                    jobs: 4,
                    png_level: 5,
                    // Keep this about scheduling; quantization is slow in debug builds.
                    lossy_png: false,
                    max_pixels: 640 * 640,
                    ..Default::default()
                },
            );
            let _ = sender.send(report.map(|r| r.resources.len()).map_err(|e| e.to_string()));
        });
    }
    for _ in 0..3 {
        let finished = receiver
            .recv_timeout(std::time::Duration::from_secs(240))
            .expect("analysis deadlocked");
        assert_eq!(finished, Ok(24));
    }
}

/// Regression: images without a smaller candidate had no preview at all, so
/// already-optimal PNGs and WebP files (without --webp) could not be viewed.
#[test]
fn every_decoded_image_gets_a_preview_even_without_candidates() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    // Already optimal: a second pass finds nothing smaller.
    let first = base.path().join("first");
    write(&project, "app/src/main/res/drawable/once.png", &png(40));
    let report = analyze(
        &project,
        &first,
        AnalysisOptions {
            jobs: 1,
            // The case being tested: nothing smaller is produced for these files.
            webp: false,
            ..Default::default()
        },
    )
    .unwrap();
    let artifact = report.resources[0].candidates[0].artifact.clone().unwrap();
    let optimal = fs::read(first.join(artifact)).unwrap();
    write(&project, "app/src/main/res/drawable/once.png", &optimal);
    let webp = webp::Encoder::from_rgba(&[120; 32 * 32 * 4], 32, 32)
        .encode_simple(false, 80.0)
        .unwrap();
    write(&project, "app/src/main/res/drawable/photo.webp", &webp);

    let out = base.path().join("second");
    let report = analyze(
        &project,
        &out,
        AnalysisOptions {
            jobs: 1,
            // The case being tested: nothing smaller is produced for these files.
            webp: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.resources.len(), 2);
    for resource in &report.resources {
        assert_eq!(resource.status, "inspected", "{:?}", resource.resource.path);
        assert!(resource.candidates.iter().all(|c| c.artifact.is_none()));
        let preview = resource
            .original_preview
            .as_ref()
            .expect("preview for every decoded image");
        assert!(fs::read(out.join(preview)).unwrap().starts_with(b"\x89PNG"));
        // Nothing smaller exists, so the source is not duplicated into the report.
        assert_eq!(resource.original_artifact, None);
        assert!(
            resource
                .issues
                .contains(&"webp_candidates_disabled".to_string())
        );
    }
}

/// Lossy PNG keeps the file a PNG, is never applied by the default (lossless)
/// batch policy, and round-trips through apply and restore like any candidate.
#[test]
fn lossy_png_candidates_are_opt_in_at_apply_time_and_restore_exactly() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("project");
    // Many distinct colours: a palette of 64–256 entries must lose something.
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 160, 160);
        encoder.set_color(png::ColorType::Rgba);
        // Stored uncompressed, like many exported assets, so savings are real.
        encoder.set_compression(png::Compression::NoCompression);
        let data: Vec<u8> = (0..160 * 160_u32)
            .flat_map(|p| {
                [
                    (p % 160) as u8,
                    (p / 160) as u8,
                    ((p % 160 + p / 160) / 2) as u8,
                    255,
                ]
            })
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
    }
    write(
        &project,
        "App/Assets.xcassets/Hero.imageset/hero.png",
        &bytes,
    );
    write(
        &project,
        "App/Assets.xcassets/Hero.imageset/Contents.json",
        br#"{"images":[{"filename":"hero.png","idiom":"universal"}],"info":{"author":"xcode","version":1}}"#,
    );
    let out = base.path().join("report");
    let report = analyze(
        &project,
        &out,
        AnalysisOptions {
            qualities: vec![75, 95],
            // Judge the mechanics here, not the score of a synthetic image.
            min_score: 0.0,
            webp: false,
            ..Default::default()
        },
    )
    .unwrap();
    let row = &report
        .resources
        .iter()
        .find(|r| r.resource.format == "png")
        .unwrap();
    let lossy: Vec<_> = row
        .candidates
        .iter()
        .filter(|c| c.format == "png" && c.lossy)
        .collect();
    assert_eq!(lossy.len(), 2, "{:?}", row.candidates);
    for candidate in &lossy {
        assert!(
            candidate.artifact.is_some() && candidate.valid,
            "{candidate:?}"
        );
        assert!(candidate.difference.as_ref().unwrap().ssimulacra2.is_some());
        assert!(
            candidate
                .notes
                .iter()
                .any(|n| n.starts_with("palette_colors: "))
        );
    }
    let (q75, q95) = (lossy[0], lossy[1]);
    assert!(q75.bytes < q95.bytes, "fewer colours should be smaller");

    // The default batch policy is lossless-only.
    let default_plan = plan_report(&out, &BatchPolicy::default()).unwrap();
    assert!(default_plan.items.iter().all(|i| !i.lossy));
    let policy = BatchPolicy {
        lossless: false,
        lossy: true,
        formats: vec!["png".into()],
        ..Default::default()
    };
    let plan = plan_report(&out, &policy).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(
        (plan.items[0].format.as_str(), plan.cross_format_items),
        ("png", 0)
    );
    let status = apply_report(&out, &policy).unwrap();
    assert_eq!(
        (status.applied, status.failed),
        (1, 0),
        "{:?}",
        status.outcomes
    );
    let source = project.join("App/Assets.xcassets/Hero.imageset/hero.png");
    let applied = fs::read(&source).unwrap();
    assert!(applied.len() < bytes.len());
    let info = png::Decoder::new(std::io::Cursor::new(&applied))
        .read_info()
        .unwrap();
    assert_eq!(
        (
            info.info().color_type,
            info.info().width,
            info.info().height
        ),
        (png::ColorType::Indexed, 160, 160)
    );
    assert_eq!(restore_report(&out).unwrap().applied, 1);
    assert_eq!(fs::read(&source).unwrap(), bytes);
}
