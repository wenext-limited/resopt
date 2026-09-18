use super::*;
use crate::{AnalysisOptions, ImageCandidate, Resource, ResourceInventory};

fn fixture(format: &str, catalog: bool) -> (tempfile::TempDir, tempfile::TempDir, Review, Vec<u8>) {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let mut original = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut original, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::NoCompression);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[36, 80, 120, 255].repeat(64 * 64))
            .unwrap();
    }
    let path = if catalog {
        PathBuf::from("Assets.xcassets/Example.imageset/picture.png")
    } else {
        PathBuf::from("picture.png")
    };
    fs::create_dir_all(root.path().join(path.parent().unwrap())).unwrap();
    fs::write(root.path().join(&path), &original).unwrap();
    if catalog {
        fs::write(root.path().join(path.parent().unwrap()).join("Contents.json"), br#"{"images":[{"filename":"picture.png","idiom":"universal","scale":"2x"},{"filename":"picture.png","idiom":"universal","scale":"3x"}],"info":{"version":1,"author":"xcode"},"custom":{"keep":true}}"#).unwrap();
    }
    let optimized = if format == "png" {
        optimizer::optimize(&original, &crate::Policy::default()).unwrap()
    } else if format == "webp" {
        crate::webp_backend::encode(
            &original,
            &image_backend::decode(&original, crate::DEFAULT_MAX_PIXELS).unwrap(),
            85,
        )
        .unwrap()
    } else {
        image_backend::encode(&original, format, 85).unwrap()
    };
    fs::create_dir(out.path().join("candidates")).unwrap();
    let artifact = PathBuf::from(format!("candidates/0.{format}"));
    fs::write(out.path().join(&artifact), &optimized).unwrap();
    let resource = Resource {
        path,
        bytes: original.len() as u64,
        kind: "image".into(),
        format: "png".into(),
        extension: "png".into(),
        extension_mismatch: false,
        origin: if catalog {
            "catalog_rendition"
        } else {
            "loose_file"
        }
        .into(),
        conversion_exclusion: None,
        format_lock: None,
        android: None,
        support: "optimizable".into(),
    };
    let report = AnalysisReport {
        schema_version: 1,
        root: fs::canonicalize(root.path()).unwrap(),
        backend: "test".into(),
        options: AnalysisOptions::default(),
        inventory: ResourceInventory {
            schema_version: 2,
            root: root.path().into(),
            catalogs: usize::from(catalog),
            assets: vec![],
            skipped_source_or_tooling_files: 0,
            excluded_directories: vec![],
            diagnostics: vec![],
            project_kinds: vec![],
            android_min_sdk: None,
        },
        resources: vec![ResourceAnalysis {
            resource,
            sha256: Some(hash(&original)),
            image: None,
            status: "candidates_available".into(),
            issues: vec![],
            candidates: vec![ImageCandidate {
                format: format.into(),
                quality: if format == "png" { None } else { Some(85) },
                lossy: format != "png",
                bytes: optimized.len() as u64,
                savings_bytes: (original.len() - optimized.len()) as u64,
                valid: true,
                rejection: None,
                difference: None,
                artifact: Some(artifact),
                preview: None,
                sha256: None,
                warnings: vec![],
                notes: vec![],
            }],
            smallest_candidate: Some(0),
            original_preview: None,
            original_artifact: None,
            fingerprint: None,
        }],
        status_counts: BTreeMap::new(),
        potential_source_bytes_saved: 0,
        similar_groups: vec![],
        cancelled: false,
        performance: None,
    };
    fs::write(
        out.path().join("analysis.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let review = Review::open(out.path()).unwrap();
    (root, out, review, original)
}

#[test]
fn lossless_apply_restart_restore_and_reapply() {
    let (root, out, review, original) = fixture("png", true);
    let source = root.path().join(&review.report.resources[0].resource.path);
    review.apply(0, 0, false).unwrap();
    assert!(fs::metadata(&source).unwrap().len() < original.len() as u64);
    assert_eq!(review.states()["0"]["state"], "applied");
    let reopened = Review::open(out.path()).unwrap();
    reopened.restore(0).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
    reopened.apply(0, 0, false).unwrap();
    reopened.restore(0).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
}

#[test]
fn changed_source_candidate_and_user_edits_are_refused() {
    let (root, out, review, original) = fixture("png", false);
    let source = root.path().join("picture.png");
    fs::write(&source, b"user edit").unwrap();
    assert!(review.apply(0, 0, false).is_err());
    fs::write(&source, &original).unwrap();
    let artifact = out.path().join("candidates/0.png");
    let candidate = fs::read(&artifact).unwrap();
    fs::write(&artifact, b"tampered").unwrap();
    assert!(review.apply(0, 0, false).is_err());
    fs::write(&artifact, candidate).unwrap();
    review.apply(0, 0, false).unwrap();
    fs::write(&source, b"later user edit").unwrap();
    assert!(review.restore(0).is_err());
    assert_eq!(fs::read(&source).unwrap(), b"later user edit");
    assert_eq!(review.states()["0"]["state"], "conflict");
}

#[test]
fn alpha_warning_requires_explicit_approval_and_still_checks_file_integrity() {
    let (root, out, mut review, _) = fixture("webp", false);
    let mut original = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut original, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::NoCompression);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[36, 80, 120, 128].repeat(64 * 64))
            .unwrap();
    }
    fs::write(root.path().join("picture.png"), &original).unwrap();
    let r = &mut review.report.resources[0];
    r.sha256 = Some(hash(&original));
    r.resource.bytes = original.len() as u64;
    r.smallest_candidate = None;
    r.candidates[0].valid = false;
    r.candidates[0].rejection = Some("alpha_error_exceeds_policy".into());
    assert!(review.preview(0, 0).is_err());
    assert!(review.apply_reviewed(0, 0, true, None).is_err());
    let alpha = vec!["alpha_error_exceeds_policy".to_string()];
    let approve = |warnings: &[String]| Approvals {
        lossy: true,
        warnings: warnings.to_vec(),
    };
    let plan = review.preview_with_warnings(0, 0, &alpha).unwrap();
    assert_eq!(plan["warning"], "alpha_error_exceeds_policy");
    assert!(
        review
            .apply_with_warnings(0, 0, &approve(&[]), plan["plan_token"].as_str(), true)
            .is_err()
    );
    // Approving a different warning, or an unknown one, does not unlock it.
    for wrong in ["quality_below_policy", "dimensions_changed"] {
        assert!(
            review
                .apply_with_warnings(
                    0,
                    0,
                    &approve(&[wrong.to_string()]),
                    plan["plan_token"].as_str(),
                    true
                )
                .is_err()
        );
    }
    review
        .apply_with_warnings(0, 0, &approve(&alpha), plan["plan_token"].as_str(), true)
        .unwrap();
    assert!(root.path().join("picture.webp").is_file());
    let transaction: serde_json::Value = serde_json::from_slice(
        &fs::read(out.path().join("operations/0/transaction.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(transaction["approved_alpha_loss"], true);
    assert_eq!(
        transaction["approved_warnings"],
        serde_json::json!(["alpha_error_exceeds_policy"])
    );
    review.restore(0).unwrap();
    assert_eq!(fs::read(root.path().join("picture.png")).unwrap(), original);
    let plan = review.preview_with_warnings(0, 0, &alpha).unwrap();
    fs::write(root.path().join("picture.png"), b"external edit").unwrap();
    assert!(
        review
            .apply_with_warnings(0, 0, &approve(&alpha), plan["plan_token"].as_str(), true)
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("picture.png")).unwrap(),
        b"external edit"
    );
}

#[test]
fn project_lock_and_catalog_exclusion_are_enforced() {
    let (root, _out, review, original) = fixture("png", true);
    let held = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.path().join(".resopt.lock"))
        .unwrap();
    held.lock().unwrap();
    assert!(review.apply(0, 0, false).is_err());
    drop(held);
    let dir = root.path().join("Assets.xcassets/Example.imageset");
    fs::write(dir.join("Contents.json"),br#"{"images":[{"filename":"picture.png","idiom":"universal"}],"properties":{"resizing":{"mode":"9-part"}}}"#).unwrap();
    assert!(review.apply(0, 0, false).is_err());
    assert_eq!(fs::read(dir.join("picture.png")).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn symlink_sources_and_backup_directories_are_refused() {
    let (root, out, review, original) = fixture("png", false);
    let external = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(external.path(), out.path().join("operations")).unwrap();
    assert!(review.apply(0, 0, false).is_err());
    assert_eq!(fs::read(root.path().join("picture.png")).unwrap(), original);
    fs::remove_file(out.path().join("operations")).unwrap();
    fs::write(external.path().join("image.png"), &original).unwrap();
    fs::remove_file(root.path().join("picture.png")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("image.png"),
        root.path().join("picture.png"),
    )
    .unwrap();
    assert!(review.apply(0, 0, false).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn jpeg_and_heic_update_all_renditions_and_restore_exact_catalog() {
    for format in ["jpeg", "heic"] {
        let (root, out, review, original) = fixture(format, true);
        let dir = root.path().join("Assets.xcassets/Example.imageset");
        let contents = fs::read(dir.join("Contents.json")).unwrap();
        assert!(review.apply(0, 0, false).is_err());
        review.apply(0, 0, true).unwrap();
        assert!(!dir.join("picture.png").exists());
        assert!(dir.join(format!("picture.{format}")).exists());
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("Contents.json")).unwrap()).unwrap();
        for image in value["images"].as_array().unwrap() {
            assert_eq!(image["filename"], format!("picture.{format}"));
        }
        assert_eq!(value["custom"]["keep"], true);
        Review::open(out.path()).unwrap().restore(0).unwrap();
        assert_eq!(fs::read(dir.join("picture.png")).unwrap(), original);
        assert_eq!(fs::read(dir.join("Contents.json")).unwrap(), contents);
        assert!(!dir.join(format!("picture.{format}")).exists());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn partial_conversion_recovers_but_collisions_and_loose_rename_are_refused() {
    let (root, _out, review, original) = fixture("heic", true);
    let dir = root.path().join("Assets.xcassets/Example.imageset");
    fs::write(dir.join("picture.heic"), b"existing image").unwrap();
    assert!(review.apply(0, 0, true).is_err());
    fs::remove_file(dir.join("picture.heic")).unwrap();
    let contents = fs::read(dir.join("Contents.json")).unwrap();
    review.apply(0, 0, true).unwrap();
    // Simulate interruption after target creation, before catalog/source changes.
    fs::write(dir.join("picture.png"), &original).unwrap();
    fs::write(dir.join("Contents.json"), &contents).unwrap();
    assert_eq!(review.states()["0"]["state"], "partial");
    review.restore(0).unwrap();
    assert!(!dir.join("picture.heic").exists());
    assert_eq!(fs::read(dir.join("picture.png")).unwrap(), original);
    let (_root, _out, review, _original) = fixture("jpeg", false);
    assert!(review.apply(0, 0, true).is_err());
}
#[cfg(target_os = "macos")]
#[test]
fn shared_catalog_conversions_restore_in_reverse_order() {
    let (root, _out, mut review, original) = fixture("heic", true);
    let dir = root.path().join("Assets.xcassets/Example.imageset");
    fs::write(dir.join("second.png"), &original).unwrap();
    let contents=br#"{"images":[{"filename":"picture.png","idiom":"universal","scale":"2x"},{"filename":"second.png","idiom":"universal","scale":"3x"}]}"#;
    fs::write(dir.join("Contents.json"), contents).unwrap();
    let mut second = review.report.resources[0].clone();
    second.resource.path = second.resource.path.with_file_name("second.png");
    review.report.resources.push(second);
    review.apply(0, 0, true).unwrap();
    review.apply(1, 0, true).unwrap();
    assert!(review.restore(0).is_err());
    assert_eq!(review.states()["0"]["state"], "conflict");
    review.restore(1).unwrap();
    review.restore(0).unwrap();
    assert_eq!(fs::read(dir.join("Contents.json")).unwrap(), contents);
    assert_eq!(fs::read(dir.join("second.png")).unwrap(), original);
}
#[cfg(target_os = "macos")]
#[test]
fn loose_cross_format_migrates_references_and_restores_them_after_restart() {
    for format in ["jpeg", "heic"] {
        let (root, out, review, original) = fixture(format, false);
        let swift = r#"let image = UIImage(named: "picture"); let u = Bundle.main.url(forResource: "picture", withExtension: "png")"#;
        let config = r#"{"image":"picture.png"}"#;
        let pbx = r#"AAAAAAAAAAAAAAAAAAAAAAAA = {isa = PBXFileReference; lastKnownFileType = image.png; path = picture.png; sourceTree = "<group>"; };"#;
        fs::write(root.path().join("View.swift"), swift).unwrap();
        fs::write(root.path().join("config.json"), config).unwrap();
        fs::create_dir(root.path().join("App.xcodeproj")).unwrap();
        fs::write(root.path().join("App.xcodeproj/project.pbxproj"), pbx).unwrap();
        fs::write(root.path().join(".gitignore"), "ignored.json\n").unwrap();
        fs::write(root.path().join("ignored.json"), config).unwrap();
        let preview = review.preview(0, 0).unwrap();
        assert_eq!(preview["reference_files"].as_array().unwrap().len(), 3);
        assert_eq!(
            preview["plan_token"],
            review.preview(0, 0).unwrap()["plan_token"]
        );
        assert!(review.apply(0, 0, true).is_err());
        review
            .apply_reviewed(0, 0, true, preview["plan_token"].as_str())
            .unwrap();
        assert!(!root.path().join("picture.png").exists());
        assert!(root.path().join(format!("picture.{format}")).exists());
        assert!(
            fs::read_to_string(root.path().join("View.swift"))
                .unwrap()
                .contains(&format!("picture.{format}"))
        );
        assert!(
            fs::read_to_string(root.path().join("App.xcodeproj/project.pbxproj"))
                .unwrap()
                .contains(&format!("image.{format}"))
        );
        assert_eq!(
            fs::read_to_string(root.path().join("ignored.json")).unwrap(),
            config
        );
        let reopened = Review::open(out.path()).unwrap();
        reopened.restore(0).unwrap();
        assert_eq!(fs::read(root.path().join("picture.png")).unwrap(), original);
        assert_eq!(
            fs::read_to_string(root.path().join("View.swift")).unwrap(),
            swift
        );
        assert_eq!(
            fs::read_to_string(root.path().join("App.xcodeproj/project.pbxproj")).unwrap(),
            pbx
        );
        assert_eq!(
            fs::read_to_string(root.path().join("config.json")).unwrap(),
            config
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn changed_reference_plan_and_later_edits_are_never_overwritten() {
    let (root, _out, review, original) = fixture("heic", false);
    let path = root.path().join("config.json");
    fs::write(&path, r#"{"image":"picture.png"}"#).unwrap();
    let preview = review.preview(0, 0).unwrap();
    fs::write(&path, r#"{"image":"picture.png","new":true}"#).unwrap();
    assert!(
        review
            .apply_reviewed(0, 0, true, preview["plan_token"].as_str())
            .is_err()
    );
    assert_eq!(fs::read(root.path().join("picture.png")).unwrap(), original);
    let preview = review.preview(0, 0).unwrap();
    review
        .apply_reviewed(0, 0, true, preview["plan_token"].as_str())
        .unwrap();
    let applied = fs::read(&path).unwrap();
    fs::write(&path, b"later user edits").unwrap();
    assert!(review.restore(0).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"later user edits");
    fs::write(&path, applied).unwrap();
    review.restore(0).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn loose_conversion_without_references_is_reviewable() {
    let (root, _out, review, original) = fixture("heic", false);
    let preview = review.preview(0, 0).unwrap();
    assert_eq!(preview["reference_files"].as_array().unwrap().len(), 0);
    review
        .apply_reviewed(0, 0, true, preview["plan_token"].as_str())
        .unwrap();
    review.restore(0).unwrap();
    assert_eq!(fs::read(root.path().join("picture.png")).unwrap(), original);
}
