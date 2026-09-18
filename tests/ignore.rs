use resopt::{ScanOptions, inventory, inventory_with_options, scan, scan_with_options};
use std::{fs, path::Path};
fn write(root: &Path, path: &str, data: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, data).unwrap();
}
fn paths(root: &Path, include_ignored: bool) -> Vec<String> {
    inventory_with_options(root, ScanOptions { include_ignored })
        .unwrap()
        .assets
        .iter()
        .map(|a| a.path.to_string_lossy().replace('\\', "/"))
        .collect()
}
#[test]
fn gitignore_nested_negations_and_git_exclude_are_default() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".git/info/exclude", b"private.json\n");
    write(root, ".gitignore", b"*.png\n!keep.png\nignored/\n");
    write(root, "nested/.gitignore", b"!rescue.png\n");
    for p in [
        "skip.png",
        "keep.png",
        "nested/rescue.png",
        "nested/drop.png",
        "ignored/keep.png",
        "private.json",
        "normal.json",
        ".hidden/data.json",
    ] {
        write(root, p, b"data");
    }
    let visible = paths(root, false);
    for path in [
        "keep.png",
        "nested/rescue.png",
        "normal.json",
        ".hidden/data.json",
    ] {
        assert!(
            visible.contains(&path.into()),
            "missing {path}: {visible:?}"
        );
    }
    for path in [
        "skip.png",
        "nested/drop.png",
        "ignored/keep.png",
        "private.json",
    ] {
        assert!(!visible.contains(&path.into()), "included {path}");
        assert!(paths(root, true).contains(&path.into()));
    }
}
#[test]
fn ignored_catalogs_and_renditions_are_filtered_consistently() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", b"Ignored.xcassets/\n**/drop.png\n");
    let json=br#"{"images":[{"filename":"keep.png","idiom":"universal","scale":"2x"},{"filename":"drop.png","idiom":"universal","scale":"3x"}],"info":{"version":1}}"#;
    for catalog in ["Assets.xcassets", "Ignored.xcassets"] {
        write(
            root,
            &format!("{catalog}/Example.imageset/Contents.json"),
            json,
        );
        write(
            root,
            &format!("{catalog}/Example.imageset/keep.png"),
            b"\x89PNG\r\n\x1a\n",
        );
        write(
            root,
            &format!("{catalog}/Example.imageset/drop.png"),
            b"\x89PNG\r\n\x1a\n",
        );
    }
    assert_eq!(scan(root).unwrap().assets.len(), 1);
    assert_eq!(
        scan_with_options(
            root,
            ScanOptions {
                include_ignored: true
            }
        )
        .unwrap()
        .assets
        .len(),
        4
    );
    assert_eq!(
        inventory(root)
            .unwrap()
            .assets
            .iter()
            .filter(|a| a.kind == "image")
            .count(),
        1
    );
    assert_eq!(
        inventory_with_options(
            root,
            ScanOptions {
                include_ignored: true
            }
        )
        .unwrap()
        .assets
        .iter()
        .filter(|a| a.kind == "image")
        .count(),
        4
    );
}
#[test]
fn parent_rules_apply_when_scanning_a_project_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", b"*.png\n");
    write(root, "sub/image.png", b"image");
    write(root, "sub/keep.json", b"{}");
    assert!(!paths(&root.join("sub"), false).contains(&"image.png".into()));
    assert!(paths(&root.join("sub"), true).contains(&"image.png".into()));
}
#[test]
fn cli_include_ignored_is_explicit_and_fixed_exclusions_remain() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", b"image.png\n");
    write(root, "image.png", b"image");
    write(root, "target/image.png", b"build");
    for (include, expected) in [(false, false), (true, true)] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_resopt"));
        command.arg("scan").arg(root).arg("--json");
        if include {
            command.arg("--include-ignored");
        }
        let result = command.output().unwrap();
        assert!(result.status.success());
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let files = report["assets"].as_array().unwrap();
        assert_eq!(files.iter().any(|a| a["path"] == "image.png"), expected);
        assert!(
            !files
                .iter()
                .any(|a| a["path"].as_str().unwrap().contains("target"))
        );
    }
}

#[test]
fn force_tracked_files_remain_visible_like_git_check_ignore() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(root)
            .status()
            .unwrap()
            .success()
    );
    write(root, ".gitignore", b"ignored/\n*.png\n");
    write(root, "ignored/keep.png", b"image");
    write(root, "ignored/drop.png", b"image");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["add", "-f", "ignored/keep.png"])
            .status()
            .unwrap()
            .success()
    );
    let visible = paths(root, false);
    assert!(visible.contains(&"ignored/keep.png".into()));
    assert!(!visible.contains(&"ignored/drop.png".into()));
    assert!(paths(&root.join("ignored"), false).contains(&"keep.png".into()));
}

#[test]
fn tracked_submodule_assets_are_not_lost_to_ignore_patterns() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("library");
    let root = temp.path().join("app");
    for path in [&library, &root] {
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
        for (key, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.invalid"),
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(["config", key, value])
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    write(&library, ".gitignore", b"generated/\n");
    write(&library, "generated/image.png", b"image");
    for args in [
        vec!["add", ".gitignore"],
        vec!["add", "-f", "generated/image.png"],
        vec!["commit", "-qm", "fixture"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&library)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let result = std::process::Command::new("git")
        .args(["-c", "protocol.file.allow=always", "-C"])
        .arg(&root)
        .args(["submodule", "add", "-q"])
        .arg(&library)
        .arg("vendor")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(paths(&root, false).contains(&"vendor/generated/image.png".into()));
}

#[test]
fn an_ignored_file_does_not_hide_sibling_catalogs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // `.DS_Store` sorts before the catalog directory; pruning on a file entry
    // used to skip every later sibling.
    write(root, ".gitignore", b".DS_Store\n");
    write(root, ".DS_Store", b"finder");
    write(
        root,
        "App/Assets.xcassets/Contents.json",
        br#"{"info":{"version":1,"author":"xcode"}}"#,
    );
    write(
        root,
        "App/Assets.xcassets/Icon.imageset/Contents.json",
        br#"{"images":[{"filename":"icon.png","idiom":"universal"}],"info":{"version":1,"author":"xcode"}}"#,
    );
    write(root, "App/Assets.xcassets/Icon.imageset/icon.png", b"png");
    let inventory = scan(root).unwrap();
    assert_eq!(inventory.catalogs, 1);
    assert_eq!(inventory.assets.len(), 1);
}
