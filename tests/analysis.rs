use resopt::{AnalysisOptions, analyze, inventory};
use std::{fs, path::Path};

fn png(alpha: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::NoCompression);
        let mut pixels = Vec::new();
        for i in 0..64 * 64 {
            pixels.extend_from_slice(&[
                (i % 255) as u8,
                80,
                100,
                if i % 2 == 0 { alpha } else { 255 },
            ]);
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    bytes
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn inventory_includes_loose_catalog_data_and_unknown_resources() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "Resources/tiny.png", &png(255));
    write(root.path(), "Resources/effect.svga", b"effect");
    write(root.path(), "Resources/video.mp4", b"video");
    write(root.path(), "Resources/sound.mp3", b"audio");
    write(root.path(), "Resources/data.zip", b"archive");
    write(root.path(), "Resources/font.otf", b"font");
    write(root.path(), "Resources/custom.xyz", b"unknown");
    write(root.path(), "Resources/config.yaml", b"runtime: true");
    write(
        root.path(),
        "Assets.xcassets/blob.dataset/Contents.json",
        br#"{"data":[{"filename":"file.bin","idiom":"universal"}]}"#,
    );
    write(
        root.path(),
        "Assets.xcassets/blob.dataset/file.bin",
        b"binary",
    );
    write(root.path(), "View.swift", b"source");
    write(root.path(), ".build/ignored.png", &png(255));
    write(root.path(), "Pods/Library/Resources/image.png", &png(255));
    let report = inventory(root.path()).unwrap();
    assert_eq!(report.assets.len(), 11);
    assert!(
        report
            .assets
            .iter()
            .any(|a| a.origin == "loose_file" && a.kind == "image")
    );
    assert!(report.assets.iter().any(|a| a.kind == "unclassified"));
    assert!(report.assets.iter().any(|a| a.path.ends_with("file.bin")));
    assert_eq!(report.skipped_source_or_tooling_files, 1);
}

#[test]
fn format_is_detected_from_bytes_even_with_wrong_extension() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "opaque.heic", &png(255));
    write(root.path(), "broken.png", b"\x00\x00\x00\x00ftyp1234567890");
    let report = inventory(root.path()).unwrap();
    let asset = report
        .assets
        .iter()
        .find(|a| a.path.ends_with("opaque.heic"))
        .unwrap();
    assert_eq!(asset.format, "png");
    assert!(asset.extension_mismatch);
}

#[test]
fn analysis_refuses_output_inside_project_and_invalid_qualities() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        analyze(
            root.path(),
            root.path().join("output"),
            AnalysisOptions::default()
        )
        .is_err()
    );
    assert!(!root.path().join("output").exists());
    assert!(
        analyze(
            root.path(),
            root.path().join("output"),
            AnalysisOptions {
                qualities: vec![0],
                ..AnalysisOptions::default()
            }
        )
        .is_err()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_inspects_small_images_and_routes_by_actual_alpha() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    let opaque = png(255);
    assert!(opaque.len() < 51200);
    let transparent = png(128);
    write(&input, "opaque.png", &opaque);
    write(&input, "transparent.png", &transparent);
    let out = root.path().join("report");
    let report = analyze(
        &input,
        &out,
        AnalysisOptions {
            qualities: vec![75, 85, 95],
            ..AnalysisOptions::default()
        },
    )
    .unwrap();
    assert_eq!(report.resources.len(), 2);
    let opaque_result = report
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("opaque.png"))
        .unwrap();
    let transparent_result = report
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("transparent.png"))
        .unwrap();
    assert!(!opaque_result.image.as_ref().unwrap().has_transparent_pixels);
    assert!(
        transparent_result
            .image
            .as_ref()
            .unwrap()
            .has_transparent_pixels
    );
    assert_eq!(
        opaque_result
            .candidates
            .iter()
            .filter(|c| c.format == "jpeg")
            .count(),
        3
    );
    assert_eq!(
        opaque_result
            .candidates
            .iter()
            .filter(|c| c.format == "heic")
            .count(),
        3
    );
    assert!(
        transparent_result
            .candidates
            .iter()
            .all(|c| c.format != "jpeg")
    );
    assert_eq!(
        transparent_result
            .candidates
            .iter()
            .filter(
                |c| c.format == "heic" && !c.warnings.iter().any(|w| w != "quality_below_policy")
            )
            .count(),
        3,
        "{:?}",
        transparent_result.candidates
    );
    assert_eq!(fs::read(input.join("opaque.png")).unwrap(), opaque);
    assert_eq!(
        fs::read(input.join("transparent.png")).unwrap(),
        transparent
    );
    assert!(out.join("report.html").exists());
    assert!(out.join("analysis.json").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn existing_heic_is_decoded_and_compared_again() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    write(&input, "source.png", &png(128));
    let status = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "heic"])
        .arg(input.join("source.png"))
        .arg("--out")
        .arg(input.join("existing.heic"))
        .output()
        .unwrap();
    assert!(status.status.success());
    fs::remove_file(input.join("source.png")).unwrap();
    let report = analyze(
        &input,
        root.path().join("report"),
        AnalysisOptions {
            qualities: vec![85],
            ..AnalysisOptions::default()
        },
    )
    .unwrap();
    let image = &report.resources[0];
    assert_eq!(image.resource.format, "heic");
    assert!(image.image.as_ref().unwrap().has_transparent_pixels);
    assert!(
        image
            .candidates
            .iter()
            .any(|c| c.format == "heic" && c.bytes > 0)
    );
    assert!(image.candidates.iter().all(|c| c.format != "jpeg"));
}

#[cfg(target_os = "macos")]
#[test]
fn corrupt_image_is_reported_without_stopping_inventory() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    write(&input, "bad.heic", b"broken");
    write(&input, "good.png", &png(255));
    let report = analyze(
        &input,
        root.path().join("report"),
        AnalysisOptions {
            probe_only: true,
            ..AnalysisOptions::default()
        },
    )
    .unwrap();
    assert_eq!(report.status_counts.get("failed"), Some(&1));
    assert_eq!(report.status_counts.get("inspected"), Some(&1));
}

#[cfg(target_os = "macos")]
#[test]
fn animated_png_is_inspected_without_flattening() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 16]).unwrap();
        writer.write_image_data(&[128; 16]).unwrap();
    }
    write(&input, "animated.png", &bytes);
    let report = analyze(
        &input,
        root.path().join("report"),
        AnalysisOptions::default(),
    )
    .unwrap();
    assert_eq!(report.resources[0].image.as_ref().unwrap().frames, 2);
    assert!(report.resources[0].candidates.is_empty());
    assert!(
        report.resources[0]
            .issues
            .iter()
            .any(|s| s == "multiple_frames_not_transcoded")
    );
}

#[cfg(target_os = "macos")]
fn flat_png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|i| {
                if (i / 64) % 2 == 0 {
                    [200, 30, 30, 255]
                } else {
                    [30, 30, 200, 255]
                }
            })
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    bytes
}

#[test]
fn analysis_rejects_out_of_range_pixel_caps_and_png_levels() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    for options in [
        AnalysisOptions {
            max_pixels: 0,
            ..AnalysisOptions::default()
        },
        AnalysisOptions {
            max_pixels: resopt::MAX_PIXELS_LIMIT + 1,
            ..AnalysisOptions::default()
        },
        AnalysisOptions {
            png_level: 7,
            ..AnalysisOptions::default()
        },
    ] {
        assert!(analyze(&project, root.path().join("out"), options).is_err());
        assert!(!root.path().join("out").exists());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_reports_ssimulacra2_for_every_compared_candidate() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    write(&input, "opaque.png", &png(255));
    write(&input, "transparent.png", &png(128));
    let report = analyze(
        &input,
        root.path().join("report"),
        AnalysisOptions::default(),
    )
    .unwrap();
    for resource in &report.resources {
        assert!(!resource.candidates.is_empty());
        for candidate in &resource.candidates {
            let score = candidate
                .difference
                .as_ref()
                .and_then(|difference| difference.ssimulacra2)
                .unwrap();
            assert!(score <= 100.0 + 1e-6, "{score}");
            if !candidate.lossy {
                assert!((score - 100.0).abs() < 0.01, "{score}");
            }
        }
    }
}

#[test]
fn default_pixel_cap_covers_a_full_screen_ipad_background() {
    const { assert!(resopt::DEFAULT_MAX_PIXELS >= 2048 * 2732) };
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_honors_the_configured_pixel_cap() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    write(&input, "image.png", &png(255));
    let status = |name: &str, max_pixels: usize| {
        let report = analyze(
            &input,
            root.path().join(name),
            AnalysisOptions {
                qualities: vec![85],
                max_pixels,
                ..AnalysisOptions::default()
            },
        )
        .unwrap();
        (
            report.resources[0].status.clone(),
            report.resources[0].issues.join(","),
        )
    };
    assert_ne!(status("fits", 64 * 64).0, "failed");
    let (state, issues) = status("capped", 64 * 64 - 1);
    assert_eq!(state, "failed");
    assert!(
        issues.contains("decoded_image_exceeds_max_pixels"),
        "{issues}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_png_candidate_uses_configured_reductions() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    write(&input, "flat.png", &flat_png(256, 256));
    let png_bytes = |name: &str, png_reductions: bool| {
        let report = analyze(
            &input,
            root.path().join(name),
            AnalysisOptions {
                qualities: vec![85],
                png_reductions,
                ..AnalysisOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.options.png_reductions, png_reductions);
        let candidate = &report.resources[0].candidates[0];
        assert_eq!(candidate.format, "png");
        assert!(candidate.valid, "{:?}", candidate.rejection);
        candidate.bytes
    };
    assert!(png_bytes("reduced", true) < png_bytes("strict", false));
}

#[test]
fn identical_images_reuse_immutable_candidates_but_keep_project_paths() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    let bytes = png(128);
    write(&input, "a.png", &bytes);
    write(&input, "nested/b.png", &bytes);
    let report = analyze(
        &input,
        root.path().join("report"),
        AnalysisOptions {
            qualities: vec![85],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.resources.len(), 2);
    let a = &report.resources[0];
    let b = &report.resources[1];
    assert_ne!(a.resource.path, b.resource.path);
    assert_eq!(a.sha256, b.sha256);
    assert!(!a.candidates.is_empty());
    for (ac, bc) in a.candidates.iter().zip(&b.candidates) {
        assert_eq!(ac.artifact, bc.artifact);
        assert_eq!(ac.bytes, bc.bytes);
        assert_eq!(ac.valid, bc.valid);
    }
    assert!(a.candidates.iter().any(|c| c.artifact.is_some()));
    assert_eq!(std::fs::read(input.join("a.png")).unwrap(), bytes);
    assert_eq!(std::fs::read(input.join("nested/b.png")).unwrap(), bytes);
}

#[test]
fn webp_is_opt_in_and_android_nine_patch_is_not_transcoded() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("project");
    write(&input, "a.png", &png(128));
    write(&input, "app/src/main/res/drawable/foo.9.png", &png(128));
    write(&input, "app/src/main/res/drawable/ordinary.png", &png(128));
    write(&input, "Assets.xcassets/Icon.imageset/file.png", &png(128));
    write(
        &input,
        "Assets.xcassets/Icon.imageset/Contents.json",
        br#"{"images":[{"filename":"file.png","idiom":"universal"}]}"#,
    );
    let report = analyze(
        &input,
        base.path().join("webp"),
        AnalysisOptions {
            webp: true,
            qualities: vec![85],
            ..Default::default()
        },
    )
    .unwrap();
    let a = report
        .resources
        .iter()
        .find(|r| r.resource.path == std::path::Path::new("a.png"))
        .unwrap();
    let catalog = report
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("Icon.imageset/file.png"))
        .unwrap();
    assert!(catalog.candidates.iter().all(|c| c.format != "webp"));
    let candidate = a
        .candidates
        .iter()
        .find(|c| c.format == "webp" && c.lossy)
        .unwrap();
    assert!(
        !candidate
            .warnings
            .iter()
            .any(|w| w != "quality_below_policy"),
        "{:?}",
        candidate.rejection
    );
    assert!(candidate.artifact.is_some());
    // The lossless WebP candidate is verified sample-for-sample.
    let lossless = a
        .candidates
        .iter()
        .find(|c| c.format == "webp" && !c.lossy)
        .unwrap();
    assert!(
        lossless.valid || lossless.artifact.is_none(),
        "{lossless:?}"
    );
    let data = std::fs::read(
        base.path()
            .join("webp")
            .join(candidate.artifact.as_ref().unwrap()),
    )
    .unwrap();
    write(&input, "input.webp", &data);
    let nine = report
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("foo.9.png"))
        .unwrap();
    // Nine-patch files keep their format: only pixel-identical PNG work is tried.
    assert!(
        nine.candidates
            .iter()
            .all(|c| c.format == "png" && !c.lossy),
        "{:?}",
        nine.candidates
    );
    assert_eq!(nine.resource.conversion_exclusion, None);
    assert_eq!(
        nine.resource.format_lock.as_deref(),
        Some("android_nine_patch")
    );
    assert_eq!(
        nine.resource.android.as_ref().unwrap().name.as_deref(),
        Some("foo")
    );
    let android = report
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("ordinary.png"))
        .unwrap();
    // Without a known minSdk, WebP is not assumed to be decodable.
    assert!(android.candidates.iter().all(|c| c.format == "png"));
    assert!(
        android
            .issues
            .contains(&"android_min_sdk_unknown".to_string())
    );
    let with_sdk = analyze(
        &input,
        base.path().join("webp-sdk"),
        AnalysisOptions {
            webp: true,
            qualities: vec![85],
            android_min_sdk: Some(21),
            ..Default::default()
        },
    )
    .unwrap();
    let android = with_sdk
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("ordinary.png"))
        .unwrap();
    assert!(android.candidates.iter().any(|c| c.format == "webp"));
    assert!(
        android
            .candidates
            .iter()
            .all(|c| matches!(c.format.as_str(), "png" | "webp"))
    );
    let nine = with_sdk
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("foo.9.png"))
        .unwrap();
    assert!(nine.candidates.iter().all(|c| c.format == "png"));
    let plain = analyze(
        &input,
        base.path().join("plain"),
        AnalysisOptions {
            qualities: vec![85],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        plain
            .resources
            .iter()
            .all(|r| r.candidates.iter().all(|c| c.format != "webp"))
    );
    let wp = plain
        .resources
        .iter()
        .find(|r| r.resource.path.ends_with("input.webp"))
        .unwrap();
    assert!(wp.image.is_some());
}

#[test]
fn alpha_rejected_candidates_keep_previews_and_downloads_without_becoming_recommended() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        encoder.set_compression(png::Compression::NoCompression);
        let pixel = [12340u16, 24680, 45678, 32700]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixel.repeat(64 * 64))
            .unwrap();
    }
    write(&input, "alpha.png", &bytes);
    let out = root.path().join("report");
    let report = analyze(
        &input,
        &out,
        AnalysisOptions {
            webp: true,
            qualities: vec![85],
            max_alpha_error: 0.0,
            ..Default::default()
        },
    )
    .unwrap();
    let resource = &report.resources[0];
    let index = resource
        .candidates
        .iter()
        .position(|c| c.format == "webp")
        .unwrap();
    let c = &resource.candidates[index];
    assert!(!c.valid);
    assert_eq!(c.rejection.as_deref(), Some("alpha_error_exceeds_policy"));
    assert!(out.join(c.artifact.as_ref().unwrap()).is_file());
    assert!(out.join(c.preview.as_ref().unwrap()).is_file());
    assert_ne!(resource.smallest_candidate, Some(index));
}
