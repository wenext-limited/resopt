//! Single-image, journaled replacements for the local report UI.
use crate::{
    AnalysisReport, ResourceAnalysis,
    analysis::WARNINGS,
    filesystem::{ProjectLock, contained_file, hash, read_verified, replace, write_new},
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
    #[serde(default)]
    references: Option<crate::references::ReferenceContext>,
    /// Kept for journals written before warnings were generalized.
    #[serde(default)]
    approved_alpha_loss: bool,
    /// Policy warnings the user explicitly accepted for this candidate.
    #[serde(default)]
    approved_warnings: Vec<String>,
}

/// Explicit user consent accompanying an apply request.
#[derive(Debug, Clone, Default)]
pub(crate) struct Approvals {
    pub lossy: bool,
    /// Warning identifiers from [`WARNINGS`] the user reviewed and accepted.
    pub warnings: Vec<String>,
}

impl Approvals {
    pub fn validate(&self) -> Result<()> {
        for warning in &self.warnings {
            ensure!(
                WARNINGS.contains(&warning.as_str()),
                "unknown warning approval: {warning}"
            );
        }
        Ok(())
    }
    fn covers(&self, warning: &str) -> bool {
        self.warnings.iter().any(|w| w == warning)
    }
}

/// Why a format-locked Android file cannot change format.
fn format_lock_explanation(lock: &str) -> &'static str {
    match lock {
        "android_nine_patch" => {
            "Nine-patch images must stay PNG: AAPT reads the 1-pixel stretch and content markers from the .9.png source. Lossless PNG optimization keeps every marker pixel."
        }
        "android_launcher_icon" => {
            "Launcher icons under mipmap-* keep their format: launchers and system UI render them outside the app, so only same-format lossless optimization is applied."
        }
        "android_raw_resource" => {
            "Files in res/raw are read as raw byte streams, so their encoded format is part of the app's contract. Only same-format lossless optimization is applied."
        }
        _ => "This resource must keep its current format.",
    }
}

type FileEdit = crate::references::Edit;
struct Prepared {
    edits: Vec<FileEdit>,
    references: Option<crate::references::ReferenceContext>,
}
impl Prepared {
    fn token(&self, resource: usize, candidate: usize) -> Result<String> {
        let edits: Vec<_> = self
            .edits
            .iter()
            .map(|(p, b, a)| (p, b.as_deref().map(hash), a.as_deref().map(hash)))
            .collect();
        Ok(hash(&serde_json::to_vec(&(
            resource,
            candidate,
            &edits,
            &self.references,
        ))?))
    }
}

impl Review {
    pub fn open(directory: &Path) -> Result<Self> {
        let directory = fs::canonicalize(directory)?;
        let report: AnalysisReport = serde_json::from_slice(&bounded_read(&contained_file(
            &directory,
            Path::new("analysis.json"),
        )?)?)?;
        ensure!(
            matches!(report.schema_version, 1 | 2),
            "unsupported analysis schema"
        );
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
                    // Reports record artifact hashes at analysis time; only
                    // older reports need them computed when opened.
                    let digest = match &candidate.sha256 {
                        Some(digest) => digest.clone(),
                        None => hash(&bounded_read(&contained_file(&directory, path)?)?),
                    };
                    artifact_hashes.insert(path.clone(), digest);
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

    fn lock(&self) -> Result<ProjectLock> {
        ProjectLock::acquire(
            self.report.root.join(".resopt.lock"),
            "project is being modified by another running resopt operation; try again when it finishes",
        )
    }

    pub(crate) fn min_sdk(&self) -> Option<u32> {
        self.report.options.android_min_sdk.or(self
            .report
            .inventory
            .android_min_sdk
            .as_ref()
            .map(|sdk| sdk.level))
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

    fn prepare(
        &self,
        index: usize,
        candidate_index: usize,
        approvals: &Approvals,
    ) -> Result<Prepared> {
        approvals.validate()?;
        let resource = self.source(index)?;
        let candidate = resource
            .candidates
            .get(candidate_index)
            .context("unknown candidate")?;
        ensure!(candidate.artifact.is_some(), "candidate not eligible");
        if !candidate.valid {
            ensure!(
                candidate.is_warning(),
                "candidate failed verification and cannot be applied: {}",
                candidate.rejection.as_deref().unwrap_or("unknown reason")
            );
            for warning in candidate.required_warnings() {
                ensure!(
                    approvals.covers(&warning),
                    "candidate has a warning that requires explicit approval: {warning}"
                );
            }
        }
        ensure!(
            !candidate.lossy || approvals.lossy,
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
                optimizer::verify(&original, &optimized, self.report.options.png_reductions)?;
            }
            "webp" if !candidate.lossy => {
                ensure!(
                    resource.resource.format == "png",
                    "lossless WebP is verified against PNG sources only"
                );
                crate::webp_backend::verify_lossless(
                    &original,
                    &optimized,
                    self.report.options.max_pixels,
                )?;
            }
            "jpeg" | "heic" | "webp" => {
                ensure!(candidate.lossy, "JPEG/HEIC/WebP requires lossy approval");
                let before = image_backend::decode(&original, self.report.options.max_pixels)?;
                let after = image_backend::decode(&optimized, self.report.options.max_pixels)?;
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
                // Both Alpha warnings describe one tradeoff: a change in presence is
                // the extreme case of an Alpha error, so either approval covers both.
                let alpha_approved = approvals.covers("alpha_error_exceeds_policy")
                    || approvals.covers("transparency_presence_changed");
                ensure!(
                    alpha_approved
                        || (delta.max_alpha_error <= self.report.options.max_alpha_error
                            && before.info.has_transparent_pixels
                                == after.info.has_transparent_pixels),
                    "alpha verification failed"
                );
                // Reports from before the perceptual threshold carry no such policy.
                ensure!(
                    self.report.schema_version == 1
                        || approvals.covers("quality_below_policy")
                        || delta
                            .ssimulacra2
                            .is_none_or(|score| score >= self.report.options.min_score),
                    "perceptual quality verification failed"
                );
                if resource.resource.android.is_some() && candidate.format == "webp" {
                    crate::android::webp_compatibility(
                        self.min_sdk(),
                        false,
                        before.info.has_transparent_pixels,
                    )
                    .map_err(|reason| {
                        anyhow::anyhow!(
                            "WebP is not supported on every API level of this app: {reason}"
                        )
                    })?;
                }
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
        if crossing && let Some(lock) = &resource.resource.format_lock {
            bail!("{}", format_lock_explanation(lock));
        }
        let android = resource.resource.android.as_ref();
        if crossing && android.is_some() {
            ensure!(
                candidate.format == "webp",
                "Android resources are only converted to WebP; JPEG and HEIC replacements are not proposed"
            );
            if !candidate.lossy {
                crate::android::webp_compatibility(self.min_sdk(), true, true).map_err(
                    |reason| anyhow::anyhow!("Lossless WebP is not supported on every API level of this app: {reason}"),
                )?;
            }
        }
        ensure!(
            candidate.format != "webp"
                || !rel.components().any(|p| Path::new(p.as_os_str())
                    .extension()
                    .is_some_and(|e| e == "xcassets")),
            "WebP is not supported as an Xcode image-set rendition"
        );
        let target = if crossing {
            rel.with_extension(&candidate.format)
        } else {
            rel.clone()
        };
        let mut edits: Vec<FileEdit> = vec![];
        let mut references = None;
        // Re-read catalog rules now, including AppIcon and cap-inset exclusions.
        let in_catalog = rel.components().any(|p| {
            Path::new(p.as_os_str())
                .extension()
                .is_some_and(|e| e == "xcassets")
        });
        if in_catalog {
            let inventory = crate::catalog::scan_with_options(
                &self.report.root,
                crate::ScanOptions {
                    include_ignored: true,
                },
            )?;
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
                let filename = rel
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("non-UTF8 filename")?;
                let replacement = target
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("non-UTF8 target")?;
                let replacement =
                    xcassets::replace_rendition_filename(&bytes, filename, replacement)?;
                edits.push((
                    asset.contents_path.clone(),
                    Some(bytes),
                    Some(replacement.contents),
                ));
            }
        } else if crossing && android.is_some_and(|a| a.area == "res") {
            // `res/` files are addressed by resource name, which a suffix change
            // keeps, so no reference is rewritten. Two files with one name in
            // the same configuration directory would fail the build instead.
            let name = android.and_then(|a| a.name.as_deref()).unwrap_or_default();
            let directory = self
                .report
                .root
                .join(rel.parent().context("resource has no parent")?);
            for entry in fs::read_dir(&directory)? {
                let sibling = entry?.file_name();
                let sibling = sibling.to_string_lossy();
                ensure!(
                    Some(sibling.as_ref()) == rel.file_name().and_then(|n| n.to_str())
                        || sibling.split('.').next() != Some(name),
                    "another file already defines the resource name {name:?} in this directory: {sibling}"
                );
            }
        } else if crossing {
            let (reference_edits, context) = crate::references::plan(
                &self.report.root,
                rel,
                &target,
                crate::ScanOptions {
                    include_ignored: self.report.options.include_ignored,
                },
            )?;
            edits.extend(reference_edits);
            references = Some(context);
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
        Ok(Prepared { edits, references })
    }

    #[cfg(test)]
    pub fn preview(&self, index: usize, candidate: usize) -> Result<serde_json::Value> {
        self.preview_with_warnings(index, candidate, &[])
    }

    /// Dry run of an application. `warnings` are the approvals the caller
    /// intends to send with the apply request.
    pub fn preview_with_warnings(
        &self,
        index: usize,
        candidate: usize,
        warnings: &[String],
    ) -> Result<serde_json::Value> {
        let prepared = self.prepare(
            index,
            candidate,
            &Approvals {
                lossy: true,
                warnings: warnings.to_vec(),
            },
        )?;
        let r = self.source(index)?;
        let c = &r.candidates[candidate];
        let target = if c.format != r.resource.format || r.resource.extension_mismatch {
            r.resource.path.with_extension(&c.format)
        } else {
            r.resource.path.clone()
        };
        let references: Vec<_> = prepared
            .edits
            .iter()
            .filter(|(path, _, _)| path != &r.resource.path && path != &target)
            .map(|(path, _, _)| path)
            .collect();
        let android = r.resource.android.as_ref().map(|a| {
            serde_json::json!({
                "area": a.area,
                "resource_type": a.res_type,
                "resource_name": a.name,
                "qualifiers": a.qualifiers,
                "min_sdk": self.min_sdk(),
                "usage": (a.area == "res" && target != r.resource.path)
                    .then(|| crate::android_refs::usage(&self.report.root, a).ok())
                    .flatten(),
            })
        });
        Ok(serde_json::json!({
            "plan_token": prepared.token(index, candidate)?,
            "source": r.resource.path,
            "target": target,
            "reference_files": references,
            "loose_conversion": prepared.references.is_some(),
            "warning": c.rejection.as_ref().filter(|_| c.is_warning()),
            "warnings": c.required_warnings(),
            "notes": c.notes,
            "android": android,
        }))
    }

    #[cfg(test)]
    pub fn apply(&self, index: usize, candidate: usize, approve_lossy: bool) -> Result<()> {
        self.apply_reviewed(index, candidate, approve_lossy, None)
    }

    #[cfg(test)]
    pub fn apply_reviewed(
        &self,
        index: usize,
        candidate_index: usize,
        approve_lossy: bool,
        token: Option<&str>,
    ) -> Result<()> {
        let approvals = Approvals {
            lossy: approve_lossy,
            warnings: vec![],
        };
        self.apply_with_warnings(index, candidate_index, &approvals, token, true)
    }

    /// Apply one candidate as a journaled transaction. With `require_preview`,
    /// a reference migration must carry the token of the plan the user saw;
    /// batch application previews the policy instead of every file.
    pub fn apply_with_warnings(
        &self,
        index: usize,
        candidate_index: usize,
        approvals: &Approvals,
        token: Option<&str>,
        require_preview: bool,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let prepared = self.prepare(index, candidate_index, approvals)?;
        if prepared.references.is_some() && require_preview {
            ensure!(
                token.is_some(),
                "preview and confirm the reference migration first"
            );
        }
        if let Some(token) = token {
            ensure!(
                token == prepared.token(index, candidate_index)?,
                "files changed after preview; review the migration again"
            );
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
            schema_version: 3,
            root: self.report.root.clone(),
            resource: index,
            candidate: candidate_index,
            changes: vec![],
            references: prepared.references,
            approved_alpha_loss: approvals
                .warnings
                .iter()
                .any(|w| w != "quality_below_policy"),
            // Record only the approval this candidate actually needed.
            approved_warnings: self.report.resources[index].candidates[candidate_index]
                .required_warnings()
                .into_iter()
                .filter(|warning| approvals.covers(warning))
                .collect(),
        };
        for (path, before, after) in prepared.edits {
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
            matches!(t.schema_version, 1..=3) && t.root == self.report.root && t.resource == index,
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
            !t.changes.is_empty()
                && t.changes.len() <= if t.references.is_some() { 1002 } else { 3 },
            "invalid transaction size"
        );
        let mut seen = std::collections::BTreeSet::new();
        for change in &t.changes {
            ensure!(seen.insert(&change.path), "duplicate transaction path");
            if change.path != *source && change.path != target && change.path != contents {
                let context = t
                    .references
                    .as_ref()
                    .context("unexpected transaction path")?;
                ensure!(
                    t.schema_version >= 2,
                    "references require transaction schema 2"
                );
                let before = blob(
                    &directory,
                    change
                        .before
                        .as_deref()
                        .context("missing reference original")?,
                )?;
                let after = blob(
                    &directory,
                    change
                        .after
                        .as_deref()
                        .context("missing reference candidate")?,
                )?;
                let expected = crate::references::rewrite(
                    &change.path,
                    std::str::from_utf8(&before)?,
                    source,
                    &target,
                    context,
                )?;
                ensure!(
                    expected.as_bytes() == after,
                    "reference backup is not an exact migration"
                );
            }
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
#[path = "review_tests.rs"]
mod tests;
