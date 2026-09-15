use resopt::{Policy, apply, create_plan, read_plan, restore, scan};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

fn policy() -> Policy {
    Policy {
        min_input_bytes: 0,
        min_savings_bytes: 1,
        min_savings_percent: 0.0,
        ..Policy::default()
    }
}

fn png_bytes(seed: u8, sixteen_bit: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(if sixteen_bit {
            png::BitDepth::Sixteen
        } else {
            png::BitDepth::Eight
        });
        encoder.set_compression(png::Compression::NoCompression);
        encoder
            .add_text_chunk("Author".into(), "resopt regression fixture".into())
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        let mut pixels = Vec::new();
        for index in 0..64 * 64 {
            // Include nonzero RGB under alpha=0 to detect unsafe alpha changes.
            if sixteen_bit {
                pixels.extend_from_slice(&[seed, 1, 24, 2, 55, 3, 0, 0]);
            } else {
                pixels.extend_from_slice(&[
                    seed,
                    (index % 16) as u8,
                    55,
                    if index % 2 == 0 { 0 } else { 255 },
                ]);
            }
        }
        writer.write_image_data(&pixels).unwrap();
    }
    bytes
}

fn asset(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let directory = root
        .join("Assets.xcassets")
        .join(format!("{name}.imageset"));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("Contents.json"),
        serde_json::to_vec_pretty(&json!({
            "images":[{"filename":"picture.png","idiom":"universal","scale":"2x"}],
            "info":{"author":"xcode","version":1},
            "custom-metadata":{"keep":true}
        }))
        .unwrap(),
    )
    .unwrap();
    let path = directory.join("picture.png");
    fs::write(&path, bytes).unwrap();
    path
}

fn fixture() -> (tempfile::TempDir, PathBuf, Vec<u8>, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let original = png_bytes(12, false);
    let path = asset(root.path(), "照片 with spaces", &original);
    let directory = root.path().join("review");
    (root, path, original, directory)
}

fn edit_plan(directory: &Path, edit: impl FnOnce(&mut Value)) {
    let path = directory.join("plan.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn decode(bytes: &[u8]) -> Vec<u8> {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buffer).unwrap();
    buffer.truncate(frame.buffer_size());
    buffer
}

#[test]
fn plan_apply_repeat_and_restore_preserve_pixels_and_catalog() {
    let (root, path, original, directory) = fixture();
    let contents = fs::read(path.with_file_name("Contents.json")).unwrap();
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    assert_eq!(plan.candidates.len(), 1, "{:?}", plan.skipped);
    assert!(plan.savings_bytes() > 1000);
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "planning must not mutate sources"
    );
    assert_eq!(apply(&directory).unwrap().changed, 1);
    let optimized = fs::read(&path).unwrap();
    assert!(optimized.len() < original.len());
    assert_eq!(decode(&original), decode(&optimized));
    assert_eq!(
        fs::read(path.with_file_name("Contents.json")).unwrap(),
        contents
    );
    assert_eq!(apply(&directory).unwrap().already_current, 1);
    let next = create_plan(root.path(), root.path().join("second-review"), policy()).unwrap();
    assert!(
        next.candidates.is_empty(),
        "repeat optimization must stabilize"
    );
    assert_eq!(restore(&directory).unwrap().changed, 1);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(restore(&directory).unwrap().already_current, 1);
    let journal = fs::read_to_string(directory.join("journal.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 4);
    assert!(!directory.join(".lock").exists());
}

#[test]
fn sixteen_bit_samples_and_hidden_rgb_survive() {
    let root = tempfile::tempdir().unwrap();
    let original = png_bytes(133, true);
    let path = asset(root.path(), "high-depth", &original);
    let directory = root.path().join("review");
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    assert_eq!(plan.candidates.len(), 1, "{:?}", plan.skipped);
    apply(&directory).unwrap();
    assert_eq!(decode(&fs::read(path).unwrap()), decode(&original));
}

#[test]
fn skips_icons_resizing_unreferenced_and_build_outputs() {
    let root = tempfile::tempdir().unwrap();
    let bytes = png_bytes(12, false);
    let allowed = asset(root.path(), "allowed", &bytes);
    fs::write(allowed.with_file_name("unreferenced.png"), &bytes).unwrap();
    let stretched = asset(root.path(), "stretched", &bytes);
    let contents = stretched.with_file_name("Contents.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&contents).unwrap()).unwrap();
    value["images"][0]["resizing"] = json!({"mode":"9-part"});
    fs::write(contents, serde_json::to_vec(&value).unwrap()).unwrap();
    asset(root.path(), "icon", &bytes);
    fs::rename(
        root.path().join("Assets.xcassets/icon.imageset"),
        root.path().join("Assets.xcassets/icon.appiconset"),
    )
    .unwrap();
    asset(&root.path().join(".build"), "excluded", &bytes);
    asset(
        &root.path().join(".worktrees/another-checkout"),
        "excluded",
        &bytes,
    );
    let inventory = scan(root.path()).unwrap();
    assert_eq!(inventory.catalogs, 1);
    assert_eq!(inventory.assets.len(), 3);
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.eligible)
            .count(),
        1
    );
    assert!(
        inventory
            .assets
            .iter()
            .any(|asset| asset.reason.as_deref() == Some("app_icon"))
    );
    assert!(
        inventory
            .assets
            .iter()
            .any(|asset| asset.reason.as_deref() == Some("resizing"))
    );
}

#[test]
fn all_sources_are_preflighted_before_any_write() {
    let (root, first, original, directory) = fixture();
    let second = asset(root.path(), "another", &png_bytes(15, false));
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    assert_eq!(plan.candidates.len(), 2);
    let stale_path = root.path().join(&plan.candidates.last().unwrap().path);
    fs::write(&stale_path, b"user edit").unwrap();
    assert!(apply(&directory).is_err());
    let untouched = if first == stale_path { second } else { first };
    assert_eq!(fs::read(&untouched).unwrap().len(), original.len());
    assert_eq!(fs::read(&stale_path).unwrap(), b"user edit");
    assert!(!directory.join("journal.jsonl").exists());
}

#[test]
fn changed_catalog_is_rejected() {
    let (root, path, original, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    fs::write(path.with_file_name("Contents.json"), b"{\"images\":[]}").unwrap();
    assert!(apply(&directory).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn corrupted_artifacts_are_rejected() {
    let (root, path, original, directory) = fixture();
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    fs::write(
        directory
            .join("candidates")
            .join(format!("{}.png", plan.candidates[0].optimized_sha256)),
        b"corrupt",
    )
    .unwrap();
    assert!(apply(&directory).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn a_rehashed_lossy_candidate_still_fails_pixel_verification() {
    let (root, path, original, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    let different = png_bytes(99, false);
    let options = oxipng::Options {
        bit_depth_reduction: false,
        color_type_reduction: false,
        palette_reduction: false,
        grayscale_reduction: false,
        interlace: None,
        ..oxipng::Options::default()
    };
    let changed = oxipng::optimize_from_memory(&different, &options).unwrap();
    let digest = format!("{:x}", Sha256::digest(&changed));
    fs::write(
        directory.join("candidates").join(format!("{digest}.png")),
        &changed,
    )
    .unwrap();
    edit_plan(&directory, |plan| {
        plan["candidates"][0]["optimized_sha256"] = json!(digest);
        plan["candidates"][0]["optimized_bytes"] = json!(changed.len());
    });
    let error = apply(&directory).unwrap_err();
    assert!(
        error.to_string().contains("decoded pixels changed"),
        "{error:#}"
    );
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn restore_never_clobbers_later_user_edits() {
    let (root, path, _, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    apply(&directory).unwrap();
    fs::write(&path, b"new artwork").unwrap();
    assert!(restore(&directory).is_err());
    assert_eq!(fs::read(path).unwrap(), b"new artwork");
}

#[test]
fn path_traversal_and_unknown_schema_are_rejected() {
    let (root, path, original, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    edit_plan(&directory, |plan| {
        plan["candidates"][0]["path"] = json!("../outside.png")
    });
    assert!(apply(&directory).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
    edit_plan(&directory, |plan| plan["schema_version"] = json!(999));
    assert!(read_plan(&directory).is_err());
}

#[test]
fn invalid_or_lossy_policy_is_not_silently_accepted() {
    assert!(toml::from_str::<Policy>("quality = 75").is_err());
    assert!(
        Policy {
            png_level: 7,
            ..policy()
        }
        .validate()
        .is_err()
    );
    assert!(
        Policy {
            min_savings_percent: f64::NAN,
            ..policy()
        }
        .validate()
        .is_err()
    );
    let (root, _, _, directory) = fixture();
    assert!(
        create_plan(
            root.path(),
            &directory,
            Policy {
                min_savings_percent: -1.0,
                ..policy()
            }
        )
        .is_err()
    );
    assert!(!directory.exists());
}

#[test]
fn excludes_broken_and_animated_pngs_with_reasons() {
    let root = tempfile::tempdir().unwrap();
    asset(root.path(), "broken", b"not png");
    let mut animated = png_bytes(12, false);
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&8_u32.to_be_bytes());
    chunk.extend_from_slice(b"acTL");
    chunk.extend_from_slice(&1_u32.to_be_bytes());
    chunk.extend_from_slice(&0_u32.to_be_bytes());
    chunk.extend_from_slice(&crc32fast::hash(&chunk[4..]).to_be_bytes());
    animated.splice(33..33, chunk);
    asset(root.path(), "animated", &animated);
    let plan = create_plan(root.path(), root.path().join("review"), policy()).unwrap();
    assert!(plan.candidates.is_empty());
    assert!(
        plan.skipped
            .values()
            .any(|reason| reason.contains("animated PNG"))
    );
    assert!(
        plan.skipped
            .values()
            .any(|reason| reason.contains("not a PNG"))
    );
}

#[test]
fn duplicate_rendition_reference_is_processed_once() {
    let (root, path, _, directory) = fixture();
    let contents = path.with_file_name("Contents.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&contents).unwrap()).unwrap();
    value["images"]
        .as_array_mut()
        .unwrap()
        .push(json!({"filename":"picture.png","idiom":"ipad"}));
    fs::write(contents, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(scan(root.path()).unwrap().assets.len(), 1);
    assert_eq!(
        create_plan(root.path(), directory, policy())
            .unwrap()
            .candidates
            .len(),
        1
    );
}

#[test]
fn existing_plan_is_never_overwritten_and_lock_is_respected() {
    let (root, _, _, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    let manifest = fs::read(directory.join("plan.json")).unwrap();
    assert!(create_plan(root.path(), &directory, policy()).is_err());
    assert_eq!(fs::read(directory.join("plan.json")).unwrap(), manifest);
    fs::write(directory.join(".lock"), b"other process").unwrap();
    assert!(apply(&directory).is_err());
    assert!(directory.join(".lock").exists());
}

#[cfg(unix)]
#[test]
fn source_and_artifact_symlinks_are_refused() {
    use std::os::unix::fs::symlink;
    let (root, path, original, directory) = fixture();
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    let external = root.path().join("external.png");
    fs::write(&external, &original).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&external, &path).unwrap();
    assert!(scan(root.path()).unwrap().assets.is_empty());
    assert!(apply(&directory).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, &original).unwrap();
    let backup = directory
        .join("originals")
        .join(format!("{}.png", plan.candidates[0].original_sha256));
    fs::remove_file(&backup).unwrap();
    symlink(&external, backup).unwrap();
    assert!(apply(&directory).is_err());
}

#[test]
fn cli_json_can_be_piped_and_errors_are_nonzero() {
    let (root, _, _, directory) = fixture();
    let binary = env!("CARGO_BIN_EXE_resopt");
    let scan = Command::new(binary)
        .args(["scan", "--json", "--catalog-only"])
        .arg(root.path())
        .output()
        .unwrap();
    assert!(scan.status.success());
    let json: Value = serde_json::from_slice(&scan.stdout).unwrap();
    assert_eq!(json["assets"].as_array().unwrap().len(), 1);
    let config = root.path().join("resopt.toml");
    fs::write(&config, toml::to_string(&policy()).unwrap()).unwrap();
    let planned = Command::new(binary)
        .arg("plan")
        .arg(root.path())
        .arg("--out")
        .arg(&directory)
        .arg("--policy")
        .arg(&config)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let json: Value = serde_json::from_slice(&planned.stdout).unwrap();
    assert_eq!(json["candidates"].as_array().unwrap().len(), 1);
    let applied = Command::new(binary)
        .arg("apply")
        .arg(&directory)
        .arg("--json")
        .output()
        .unwrap();
    assert!(applied.status.success());
    let restored = Command::new(binary)
        .arg("restore")
        .arg(&directory)
        .arg("--json")
        .output()
        .unwrap();
    assert!(restored.status.success());
    let missing = Command::new(binary)
        .arg("apply")
        .arg(root.path().join("absent"))
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
}

#[test]
fn interrupted_batch_can_be_restored_from_hashes() {
    let (root, first, original, directory) = fixture();
    asset(root.path(), "second", &png_bytes(19, false));
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    let item = &plan.candidates[0];
    // Simulate termination after the first replacement, before a journal entry.
    fs::copy(
        directory
            .join("candidates")
            .join(format!("{}.png", item.optimized_sha256)),
        root.path().join(&item.path),
    )
    .unwrap();
    let report = restore(&directory).unwrap();
    assert_eq!(report.changed, 1);
    assert_eq!(report.already_current, 1);
    assert_eq!(fs::read(first).unwrap(), original);
}

#[test]
fn separate_plans_respect_project_lock() {
    let (root, _, _, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    fs::write(root.path().join(".resopt.lock"), b"another plan").unwrap();
    assert!(apply(&directory).is_err());
    assert!(!directory.join(".lock").exists());
    assert!(root.path().join(".resopt.lock").exists());
}

#[cfg(unix)]
#[test]
fn dangling_journal_symlink_cannot_create_external_file() {
    let (root, path, original, directory) = fixture();
    create_plan(root.path(), &directory, policy()).unwrap();
    let external = root.path().join("must-not-create");
    std::os::unix::fs::symlink(&external, directory.join("journal.jsonl")).unwrap();
    assert!(apply(&directory).is_err());
    assert!(!external.exists());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn removed_metadata_is_rejected_even_with_updated_hash() {
    let (root, path, original, directory) = fixture();
    let plan = create_plan(root.path(), &directory, policy()).unwrap();
    let blob = directory
        .join("candidates")
        .join(format!("{}.png", plan.candidates[0].optimized_sha256));
    let mut bytes = fs::read(blob).unwrap();
    let mut offset = 8;
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        if &bytes[offset + 4..offset + 8] == b"tEXt" {
            bytes.drain(offset..offset + length + 12);
            break;
        }
        offset += length + 12;
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    fs::write(
        directory.join("candidates").join(format!("{digest}.png")),
        &bytes,
    )
    .unwrap();
    edit_plan(&directory, |plan| {
        plan["candidates"][0]["optimized_sha256"] = json!(digest);
        plan["candidates"][0]["optimized_bytes"] = json!(bytes.len());
    });
    assert!(
        apply(&directory)
            .unwrap_err()
            .to_string()
            .contains("non-IDAT chunks changed")
    );
    assert_eq!(fs::read(path).unwrap(), original);
}
