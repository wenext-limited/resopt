//! Single-image, journaled replacements for the local report UI.
use crate::{
    AnalysisReport, ResourceAnalysis,
    filesystem::{contained_file, hash, read_verified, replace, write_new},
    image_backend, optimizer,
    resources::bounded_read,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct Review {
    pub directory: PathBuf,
    pub report: AnalysisReport,
    artifact_hashes: BTreeMap<PathBuf, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    schema_version: u32,
    root: PathBuf,
    resource: usize,
    candidate: usize,
    changes: Vec<Change>,
}

type FileEdit = (PathBuf, Option<Vec<u8>>, Option<Vec<u8>>);

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Review {
    pub fn open(directory: &Path) -> Result<Self> {
        let directory = fs::canonicalize(directory)?;
        let report: AnalysisReport = serde_json::from_slice(&bounded_read(&contained_file(
            &directory,
            Path::new("analysis.json"),
        )?)?)?;
        ensure!(report.schema_version == 1, "unsupported analysis schema");
        ensure!(
            report.root.is_absolute() && fs::canonicalize(&report.root)? == report.root,
            "project root moved"
        );
        ensure!(
            !directory.starts_with(&report.root),
            "report must be outside project"
        );
        let mut artifact_hashes = BTreeMap::new();
        for resource in &report.resources {
            for candidate in &resource.candidates {
                if let Some(path) = &candidate.artifact {
                    ensure!(path.starts_with("candidates"), "invalid artifact directory");
                    artifact_hashes.insert(
                        path.clone(),
                        hash(&bounded_read(&contained_file(&directory, path)?)?),
                    );
                }
            }
        }
        Ok(Self {
            directory,
            report,
            artifact_hashes,
        })
    }

    fn source(&self, index: usize) -> Result<&ResourceAnalysis> {
        self.report.resources.get(index).context("unknown resource")
    }

    fn operation(&self, index: usize) -> Result<PathBuf> {
        self.source(index)?;
        Ok(self.directory.join("operations").join(index.to_string()))
    }

    fn lock(&self) -> Result<Lock> {
        let path = self.report.root.join(".resopt.lock");
        write_new(&path, format!("pid={}\n", std::process::id()).as_bytes())
            .context("project locked by another operation")?;
        Ok(Lock(path))
    }

    pub fn states(&self) -> serde_json::Value {
        let mut states = serde_json::Map::new();
        for index in 0..self.report.resources.len() {
            let path = self
                .directory
                .join("operations")
                .join(index.to_string())
                .join("transaction.json");
            if fs::symlink_metadata(path).is_ok() {
                let value = match self
                    .load(index)
                    .and_then(|t| Ok((self.state(&t)?, t.candidate)))
                {
                    Ok((state, candidate)) => {
                        serde_json::json!({"state":state,"candidate":candidate})
                    }
                    Err(error) => {
                        serde_json::json!({"state":"conflict", "error":format!("{error:#}")})
                    }
                };
                states.insert(index.to_string(), value);
            }
        }
        serde_json::Value::Object(states)
    }

    pub fn apply(&self, index: usize, candidate_index: usize, approve_lossy: bool) -> Result<()> {
        let _lock = self.lock()?;
        let resource = self.source(index)?;
        let candidate = resource
            .candidates
            .get(candidate_index)
            .context("unknown candidate")?;
        ensure!(
            candidate.valid && candidate.artifact.is_some(),
            "candidate not eligible"
        );
        ensure!(
            !candidate.lossy || approve_lossy,
            "lossy candidate requires explicit approval"
        );
        ensure!(
            resource.resource.conversion_exclusion.is_none(),
            "resource excluded from conversion"
        );
        ensure!(
            resource.status == "candidates_available",
            "resource has no eligible candidates"
        );
        let source = contained_file(&self.report.root, &resource.resource.path)?;
        let original = read_verified(
            &source,
            resource
                .sha256
                .as_deref()
                .context("missing original hash")?,
        )?;
        let artifact = candidate.artifact.as_ref().context("no artifact")?;
        let optimized = read_verified(
            &contained_file(&self.directory, artifact)?,
            self.artifact_hashes
                .get(artifact)
                .context("unknown artifact")?,
        )?;
        ensure!(
            optimized.len() < original.len() && optimized.len() as u64 == candidate.bytes,
            "candidate size mismatch"
        );
        match candidate.format.as_str() {
            "png" => {
                ensure!(!candidate.lossy, "PNG must be lossless");
                optimizer::verify(&original, &optimized)?;
            }
            "jpeg" | "heic" => {
                ensure!(candidate.lossy, "JPEG/HEIC requires lossy approval");
                let before = image_backend::decode(&original)?;
                let after = image_backend::decode(&optimized)?;
                ensure!(
                    before.info.frames == 1 && after.info.frames == 1,
                    "animated images cannot be applied"
                );
                let delta = image_backend::compare(&before, &after)?;
                ensure!(
                    self.report.options.max_alpha_error.is_finite()
                        && (0.0..=1.0).contains(&self.report.options.max_alpha_error),
                    "invalid alpha policy"
                );
                ensure!(
                    delta.max_alpha_error <= self.report.options.max_alpha_error
                        && before.info.has_transparent_pixels == after.info.has_transparent_pixels,
                    "alpha verification failed"
                );
                ensure!(
                    candidate.format != "jpeg" || !before.info.has_transparent_pixels,
                    "JPEG cannot preserve alpha"
                );
                ensure!(
                    crate::resources::actual_format(&optimized) == Some(candidate.format.as_str()),
                    "candidate format mismatch"
                );
            }
            _ => bail!("unsupported candidate format"),
        }
        let rel = &resource.resource.path;
        let crossing =
            candidate.format != resource.resource.format || resource.resource.extension_mismatch;
        let target = if crossing {
            rel.with_extension(&candidate.format)
        } else {
            rel.clone()
        };
        let mut edits: Vec<FileEdit> = vec![];
        // Re-read catalog rules now, including AppIcon and cap-inset exclusions.
        let in_catalog = rel.components().any(|p| {
            Path::new(p.as_os_str())
                .extension()
                .is_some_and(|e| e == "xcassets")
        });
        if in_catalog {
            let inventory = crate::catalog::scan(&self.report.root)?;
            let asset = inventory
                .assets
                .iter()
                .find(|a| a.path == *rel)
                .context("image is not a supported catalog rendition")?;
            ensure!(
                asset.reason.is_none() || asset.reason.as_deref() == Some("unsupported_format"),
                "catalog resource excluded"
            );
            let contents = contained_file(&self.report.root, &asset.contents_path)?;
            let bytes = read_verified(&contents, &asset.contents_sha256)?;
            if crossing {
                let mut json: serde_json::Value = serde_json::from_slice(&bytes)?;
                let filename = rel
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("non-UTF8 filename")?;
                let replacement = target
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("non-UTF8 target")?;
                let images = json["images"]
                    .as_array_mut()
                    .context("missing catalog images")?;
                ensure!(
                    !images
                        .iter()
                        .any(|image| image["filename"] == replacement
                            && image["filename"] != filename),
                    "target already referenced"
                );
                let mut count = 0;
                for image in images {
                    if image["filename"] == filename {
                        image["filename"] = replacement.into();
                        count += 1;
                    }
                }
                ensure!(count > 0, "source no longer referenced");
                edits.push((
                    asset.contents_path.clone(),
                    Some(bytes),
                    Some(serde_json::to_vec_pretty(&json)?),
                ));
            }
        } else {
            ensure!(
                !crossing,
                "loose-file format conversion needs reference migration; use a same-format candidate"
            );
        }
        if target != *rel {
            ensure!(
                current(&self.report.root, &target)?.is_none(),
                "target filename already exists"
            );
            edits.insert(0, (target, None, Some(optimized)));
            edits.push((rel.clone(), Some(original), None));
        } else {
            edits.insert(0, (rel.clone(), Some(original), Some(optimized)));
        }
        let directory = self.operation(index)?;
        if fs::symlink_metadata(directory.join("transaction.json")).is_ok() {
            ensure!(
                self.state(&self.load(index)?)? == "original",
                "restore the previous operation first"
            );
        }
        safe_directory(&self.directory, Path::new("operations"))?;
        safe_directory(
            &self.directory,
            &Path::new("operations").join(index.to_string()),
        )?;
        let mut transaction = Transaction {
            schema_version: 1,
            root: self.report.root.clone(),
            resource: index,
            candidate: candidate_index,
            changes: vec![],
        };
        for (path, before, after) in edits {
            let save = |data: Option<Vec<u8>>| -> Result<Option<String>> {
                data.map(|bytes| {
                    let digest = hash(&bytes);
                    let path = directory.join(format!("{digest}.bin"));
                    if fs::symlink_metadata(&path).is_ok() {
                        let file = contained_file(&directory, Path::new(&format!("{digest}.bin")))?;
                        read_verified(&file, &digest)?;
                    } else {
                        write_new(&path, &bytes)?;
                    }
                    Ok(digest)
                })
                .transpose()
            };
            transaction.changes.push(Change {
                path,
                before: save(before)?,
                after: save(after)?,
            });
        }
        self.check(&transaction)?;
        let manifest = directory.join("transaction.json");
        let data = serde_json::to_vec_pretty(&transaction)?;
        if fs::symlink_metadata(&manifest).is_ok() {
            contained_file(&directory, Path::new("transaction.json"))?;
            replace(&manifest, &data)?;
        } else {
            write_new(&manifest, &data)?;
        }
        // The durable manifest is written before the first source change. On any
        // interruption, restore accepts a mix of original and applied files.
        for change in &transaction.changes {
            self.write(&directory, change, false)?;
        }
        ensure!(
            self.state(&transaction)? == "applied",
            "apply incomplete; restore from report"
        );
        Ok(())
    }

    pub fn restore(&self, index: usize) -> Result<()> {
        let _lock = self.lock()?;
        let transaction = self.load(index)?;
        self.check(&transaction)?;
        for change in transaction.changes.iter().rev() {
            self.write(&self.operation(index)?, change, true)?;
        }
        ensure!(
            self.state(&transaction)? == "original",
            "restore incomplete"
        );
        Ok(())
    }

    fn load(&self, index: usize) -> Result<Transaction> {
        let directory = self.operation(index)?;
        let relative = Path::new("operations")
            .join(index.to_string())
            .join("transaction.json");
        let t: Transaction =
            serde_json::from_slice(&bounded_read(&contained_file(&self.directory, &relative)?)?)?;
        ensure!(
            t.schema_version == 1 && t.root == self.report.root && t.resource == index,
            "invalid transaction"
        );
        let r = self.source(index)?;
        let c = r
            .candidates
            .get(t.candidate)
            .context("invalid recorded candidate")?;
        let source = &r.resource.path;
        let target = source.with_extension(&c.format);
        let contents = source.parent().context("no parent")?.join("Contents.json");
        ensure!(
            !t.changes.is_empty() && t.changes.len() <= 3,
            "invalid transaction size"
        );
        let mut seen = std::collections::BTreeSet::new();
        for change in &t.changes {
            ensure!(seen.insert(&change.path), "duplicate transaction path");
            ensure!(
                change.path == *source || change.path == target || change.path == contents,
                "unexpected transaction path"
            );
            for digest in [&change.before, &change.after].into_iter().flatten() {
                blob(&directory, digest)?;
            }
        }
        Ok(t)
    }

    fn check(&self, t: &Transaction) -> Result<()> {
        for change in &t.changes {
            let actual = current(&self.report.root, &change.path)?;
            ensure!(
                actual == change.before || actual == change.after,
                "file modified since operation: {}; restore newer operations first",
                change.path.display()
            );
        }
        Ok(())
    }
    fn state(&self, t: &Transaction) -> Result<&'static str> {
        self.check(t)?;
        let mut original = true;
        let mut applied = true;
        for c in &t.changes {
            let actual = current(&self.report.root, &c.path)?;
            original &= actual == c.before;
            applied &= actual == c.after;
        }
        Ok(if original {
            "original"
        } else if applied {
            "applied"
        } else {
            "partial"
        })
    }
    fn write(&self, directory: &Path, c: &Change, restoring: bool) -> Result<()> {
        let actual = current(&self.report.root, &c.path)?;
        ensure!(
            actual == c.before || actual == c.after,
            "file changed before write: {}",
            c.path.display()
        );
        let wanted = if restoring { &c.before } else { &c.after };
        if &actual == wanted {
            return Ok(());
        }
        let path = self.report.root.join(&c.path);
        if let Some(digest) = wanted {
            let bytes = blob(directory, digest)?;
            if actual.is_some() {
                replace(&path, &bytes)?;
            } else {
                write_new(&path, &bytes)?;
            }
        } else {
            fs::remove_file(path)?;
        }
        ensure!(
            &current(&self.report.root, &c.path)? == wanted,
            "write verification failed"
        );
        Ok(())
    }
}

fn blob(directory: &Path, digest: &str) -> Result<Vec<u8>> {
    ensure!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid digest"
    );
    read_verified(
        &contained_file(directory, Path::new(&format!("{digest}.bin")))?,
        digest,
    )
}

fn safe_directory(root: &Path, relative: &Path) -> Result<()> {
    let path = root.join(relative);
    if fs::symlink_metadata(&path).is_err() {
        fs::create_dir(&path)?;
    }
    ensure!(
        !fs::symlink_metadata(&path)?.file_type().is_symlink() && path.is_dir(),
        "unsafe operation directory"
    );
    Ok(())
}

/// Like contained_file, but permits only the final component to be absent.
fn current(root: &Path, relative: &Path) -> Result<Option<String>> {
    let mut path = root.to_path_buf();
    let parts: Vec<_> = relative.components().collect();
    ensure!(!parts.is_empty(), "empty path");
    for (i, part) in parts.iter().enumerate() {
        let std::path::Component::Normal(name) = part else {
            bail!("unsafe path");
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) => ensure!(!meta.file_type().is_symlink(), "symlink refused"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && i == parts.len() - 1 => {
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(Some(hash(&bounded_read(&path)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisOptions, ImageCandidate, Resource, ResourceInventory};

    fn fixture(
        format: &str,
        catalog: bool,
    ) -> (tempfile::TempDir, tempfile::TempDir, Review, Vec<u8>) {
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
                }],
                smallest_candidate: Some(0),
                original_preview: None,
                original_artifact: None,
            }],
            status_counts: BTreeMap::new(),
            potential_source_bytes_saved: 0,
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
    fn project_lock_and_catalog_exclusion_are_enforced() {
        let (root, _out, review, original) = fixture("png", true);
        fs::write(root.path().join(".resopt.lock"), b"other process").unwrap();
        assert!(review.apply(0, 0, false).is_err());
        fs::remove_file(root.path().join(".resopt.lock")).unwrap();
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
}
