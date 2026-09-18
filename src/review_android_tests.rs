//! End-to-end Android application rules, driven through a real analysis.
use super::*;
use crate::{AnalysisOptions, analyze};

fn gradient_png(alpha: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 96, 96);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_compression(png::Compression::NoCompression);
    let mut data = Vec::new();
    for y in 0..96_u32 {
        for x in 0..96_u32 {
            let a = if alpha && x < 8 { 0 } else { 255 };
            data.extend([(x * 2) as u8, (y * 2) as u8, 120, a]);
        }
    }
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&data)
        .unwrap();
    bytes
}

/// A valid nine-patch: a transparent 1-pixel frame with black stretch and
/// content markers, around the gradient.
fn nine_patch_png() -> Vec<u8> {
    let size = 34_u32;
    let mut data = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let border = x == 0 || y == 0 || x == size - 1 || y == size - 1;
            let corner = (x == 0 || x == size - 1) && (y == 0 || y == size - 1);
            let marker = !corner && ((10..24).contains(&x) || (10..24).contains(&y));
            data.extend(match (border, marker) {
                (true, true) => [0, 0, 0, 255],
                (true, false) => [0, 0, 0, 0],
                _ => [(x * 7) as u8, (y * 7) as u8, 90, 255],
            });
        }
    }
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_compression(png::Compression::NoCompression);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&data)
        .unwrap();
    bytes
}

struct Project {
    _base: tempfile::TempDir,
    root: PathBuf,
    review: Review,
}

fn project(min_sdk: Option<u32>, extra: &[(&str, Vec<u8>)]) -> Project {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("project");
    let mut files = vec![
        ("app/src/main/AndroidManifest.xml", b"<manifest/>".to_vec()),
        (
            "app/src/main/res/drawable-xxhdpi/bg_home.png",
            gradient_png(true),
        ),
        (
            "app/src/main/res/drawable-xhdpi/bubble.9.png",
            nine_patch_png(),
        ),
        (
            "app/src/main/res/mipmap-xxhdpi/ic_launcher.png",
            gradient_png(false),
        ),
        ("app/src/main/res/raw/splash.png", gradient_png(false)),
        ("app/src/main/assets/web/logo.png", gradient_png(false)),
        (
            "app/src/main/assets/web/index.html",
            br#"<img src="logo.png">"#.to_vec(),
        ),
        (
            "app/src/main/java/Main.kt",
            br#"val a = R.drawable.bg_home; val s = assets.open("web/logo.png")"#.to_vec(),
        ),
        (
            "app/src/main/res/layout/main.xml",
            br#"<ImageView android:src="@drawable/bg_home"/>"#.to_vec(),
        ),
    ];
    files.extend(extra.iter().map(|(p, b)| (*p, b.clone())));
    for (path, bytes) in files {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }
    let out = base.path().join("report");
    analyze(
        &root,
        &out,
        AnalysisOptions {
            webp: true,
            qualities: vec![90],
            min_score: 0.0,
            android_min_sdk: min_sdk,
            jobs: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let review = Review::open(&out).unwrap();
    Project {
        root: fs::canonicalize(&root).unwrap(),
        _base: base,
        review,
    }
}

impl Project {
    fn find(&self, suffix: &str, format: &str, lossy: bool) -> (usize, usize) {
        let (index, resource) = self
            .review
            .report
            .resources
            .iter()
            .enumerate()
            .find(|(_, r)| r.resource.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("{suffix} not in report"));
        let candidate = resource
            .candidates
            .iter()
            .position(|c| c.format == format && c.lossy == lossy && c.artifact.is_some())
            .unwrap_or_else(|| panic!("no {format} candidate for {suffix}: {resource:?}"));
        (index, candidate)
    }
    fn apply(&self, index: usize, candidate: usize) -> Result<()> {
        let plan = self.review.preview(index, candidate)?;
        self.review
            .apply_reviewed(index, candidate, true, plan["plan_token"].as_str())
    }
}

#[test]
fn drawable_png_to_webp_keeps_the_resource_name_and_restores_exactly() {
    let p = project(Some(21), &[]);
    let source = p.root.join("app/src/main/res/drawable-xxhdpi/bg_home.png");
    let original = fs::read(&source).unwrap();
    let kotlin = fs::read(p.root.join("app/src/main/java/Main.kt")).unwrap();
    let (index, candidate) = p.find("bg_home.png", "webp", false);
    let plan = p.review.preview(index, candidate).unwrap();
    assert_eq!(plan["android"]["resource_name"], "bg_home");
    assert_eq!(plan["android"]["min_sdk"], 21);
    assert_eq!(plan["android"]["usage"]["xml_references"], 1);
    assert_eq!(plan["android"]["usage"]["code_references"], 1);
    // The name is unchanged, so no source or XML file is rewritten.
    assert_eq!(plan["reference_files"].as_array().unwrap().len(), 0);
    p.apply(index, candidate).unwrap();
    assert!(!source.exists());
    let target = source.with_extension("webp");
    crate::webp_backend::verify_lossless(&original, &fs::read(&target).unwrap(), 1 << 20).unwrap();
    assert_eq!(
        fs::read(p.root.join("app/src/main/java/Main.kt")).unwrap(),
        kotlin
    );
    p.review.restore(index).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(!target.exists());
}

#[test]
fn a_second_file_with_the_same_resource_name_blocks_the_conversion() {
    let p = project(
        Some(21),
        &[(
            "app/src/main/res/drawable-xxhdpi/bg_home.jpg",
            b"jpeg".to_vec(),
        )],
    );
    let (index, candidate) = p.find("bg_home.png", "webp", true);
    let error = p.apply(index, candidate).unwrap_err().to_string();
    assert!(
        error.contains("already defines the resource name"),
        "{error}"
    );
    assert!(
        p.root
            .join("app/src/main/res/drawable-xxhdpi/bg_home.png")
            .exists()
    );
}

#[test]
fn nine_patch_launcher_and_raw_files_only_get_lossless_same_format_work() {
    let p = project(Some(21), &[]);
    for name in ["bubble.9.png", "ic_launcher.png", "splash.png"] {
        let resource = p
            .review
            .report
            .resources
            .iter()
            .find(|r| r.resource.path.ends_with(name))
            .unwrap();
        assert!(resource.resource.format_lock.is_some(), "{name}");
        assert!(
            resource
                .candidates
                .iter()
                .all(|c| c.format == "png" && !c.lossy),
            "{name}: {:?}",
            resource.candidates
        );
    }
    // Lossless PNG work on a nine-patch keeps every pixel, including the
    // one-pixel stretch and content markers.
    let (index, candidate) = p.find("bubble.9.png", "png", false);
    let source = p.root.join("app/src/main/res/drawable-xhdpi/bubble.9.png");
    let original = fs::read(&source).unwrap();
    p.apply(index, candidate).unwrap();
    let optimized = fs::read(&source).unwrap();
    assert!(optimized.len() < original.len());
    crate::png_pixels::ensure_same_rgba(&original, &optimized, 1 << 26).unwrap();
    p.review.restore(index).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
}

#[test]
fn a_forged_cross_format_candidate_for_a_locked_file_is_refused_with_the_reason() {
    let mut p = project(Some(21), &[]);
    // Forge a structurally valid WebP candidate for the nine-patch, as a
    // tampered report or an old tool version might contain.
    let (nine, _) = p.find("bubble.9.png", "png", false);
    let original = fs::read(p.root.join("app/src/main/res/drawable-xhdpi/bubble.9.png")).unwrap();
    let decoded = image_backend::decode(&original, crate::DEFAULT_MAX_PIXELS).unwrap();
    let encoded = crate::webp_backend::encode(&original, &decoded, 90).unwrap();
    assert!(encoded.len() < original.len());
    let artifact = PathBuf::from("candidates/9999-webp-90.webp");
    fs::write(p.review.directory.join(&artifact), &encoded).unwrap();
    let mut forged = p.review.report.resources[nine].candidates[0].clone();
    forged.format = "webp".into();
    forged.quality = Some(90);
    forged.lossy = true;
    forged.bytes = encoded.len() as u64;
    forged.sha256 = Some(hash(&encoded));
    forged.artifact = Some(artifact.clone());
    p.review.artifact_hashes.insert(artifact, hash(&encoded));
    p.review.report.resources[nine].candidates.push(forged);
    let index = p.review.report.resources[nine].candidates.len() - 1;
    let error = p.apply(nine, index).unwrap_err().to_string();
    assert!(error.contains("Nine-patch images must stay PNG"), "{error}");
    assert!(
        p.root
            .join("app/src/main/res/drawable-xhdpi/bubble.9.png")
            .exists()
    );
}

#[test]
fn assets_are_path_addressed_so_references_are_migrated_and_restored() {
    let p = project(Some(21), &[]);
    let (index, candidate) = p.find("web/logo.png", "webp", true);
    let html = p.root.join("app/src/main/assets/web/index.html");
    let kotlin = p.root.join("app/src/main/java/Main.kt");
    let before = (fs::read(&html).unwrap(), fs::read(&kotlin).unwrap());
    let plan = p.review.preview(index, candidate).unwrap();
    assert_eq!(plan["loose_conversion"], true);
    p.apply(index, candidate).unwrap();
    assert!(fs::read_to_string(&html).unwrap().contains("logo.webp"));
    assert!(
        fs::read_to_string(&kotlin)
            .unwrap()
            .contains("web/logo.webp")
    );
    // The drawable reference in the same file is untouched.
    assert!(
        fs::read_to_string(&kotlin)
            .unwrap()
            .contains("R.drawable.bg_home")
    );
    p.review.restore(index).unwrap();
    assert_eq!(
        (fs::read(&html).unwrap(), fs::read(&kotlin).unwrap()),
        before
    );
}

#[test]
fn webp_is_not_offered_when_min_sdk_is_unknown_or_too_low() {
    for (min_sdk, reason) in [
        (None, "android_min_sdk_unknown"),
        (Some(16), "android_min_sdk_16_below_webp_requirement_18"),
    ] {
        let p = project(min_sdk, &[]);
        let resource = p
            .review
            .report
            .resources
            .iter()
            .find(|r| r.resource.path.ends_with("bg_home.png"))
            .unwrap();
        assert!(resource.candidates.iter().all(|c| c.format == "png"));
        assert!(
            resource.issues.iter().any(|i| i == reason),
            "{:?}",
            resource.issues
        );
    }
}
