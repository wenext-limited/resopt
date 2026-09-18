use crate::{
    Policy,
    catalog::scan_with_options,
    filesystem::{contained_file, hash, read_verified, replace, write_new},
    optimizer,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub path: PathBuf,
    pub original_sha256: String,
    pub optimized_sha256: String,
    pub original_bytes: u64,
    pub optimized_bytes: u64,
    pub contents_path: PathBuf,
    pub contents_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub root: PathBuf,
    pub backend: String,
    pub policy: Policy,
    pub candidates: Vec<Candidate>,
    pub skipped: BTreeMap<PathBuf, String>,
    pub diagnostics: Vec<String>,
}

impl Plan {
    pub fn savings_bytes(&self) -> u64 {
        self.candidates
            .iter()
            .map(|item| item.original_bytes.saturating_sub(item.optimized_bytes))
            .sum()
    }
}

/// Creates a new, self-contained plan directory. Sources are never modified.
/// A partial directory may remain if IO fails; it cannot apply without plan.json.
pub fn create_plan(
    root: impl AsRef<Path>,
    directory: impl AsRef<Path>,
    policy: Policy,
) -> Result<Plan> {
    policy.validate()?;
    let inventory = scan_with_options(
        root,
        crate::ScanOptions {
            include_ignored: policy.include_ignored,
        },
    )?;
    let directory = directory.as_ref();
    fs::create_dir(directory).with_context(|| {
        format!(
            "creating new plan directory {}; it must not already exist",
            directory.display()
        )
    })?;
    fs::create_dir(directory.join("originals"))?;
    fs::create_dir(directory.join("candidates"))?;
    let mut plan = Plan {
        schema_version: 1,
        root: inventory.root,
        backend: "oxipng/10.2.1; strict-png/1".into(),
        policy,
        candidates: vec![],
        skipped: BTreeMap::new(),
        diagnostics: inventory.diagnostics,
    };
    for asset in inventory.assets {
        if let Some(reason) = asset.reason {
            plan.skipped.insert(asset.path, reason);
            continue;
        }
        let result = (|| -> Result<()> {
            ensure!(
                asset.bytes >= plan.policy.min_input_bytes,
                "below_input_threshold"
            );
            let path = contained_file(&plan.root, &asset.path)?;
            let original = read_bounded(&path)?;
            let candidate = optimizer::optimize(&original, &plan.policy)?;
            ensure!(candidate.len() < original.len(), "not_smaller");
            let saving = (original.len() - candidate.len()) as u64;
            ensure!(
                saving >= plan.policy.min_savings_bytes
                    && saving as f64 * 100.0 / original.len() as f64
                        >= plan.policy.min_savings_percent,
                "below_savings_threshold"
            );
            // Check source and catalog remained stable during encoding.
            read_verified(&path, &hash(&original))?;
            read_verified(
                &contained_file(&plan.root, &asset.contents_path)?,
                &asset.contents_sha256,
            )?;
            let original_hash = hash(&original);
            let candidate_hash = hash(&candidate);
            save_blob(directory, "originals", &original_hash, &original)?;
            save_blob(directory, "candidates", &candidate_hash, &candidate)?;
            plan.candidates.push(Candidate {
                path: asset.path.clone(),
                original_sha256: original_hash,
                optimized_sha256: candidate_hash,
                original_bytes: original.len() as u64,
                optimized_bytes: candidate.len() as u64,
                contents_path: asset.contents_path,
                contents_sha256: asset.contents_sha256,
            });
            Ok(())
        })();
        if let Err(error) = result {
            plan.skipped.insert(asset.path, format!("{error:#}"));
        }
    }
    plan.candidates.sort_by(|a, b| {
        (b.original_bytes - b.optimized_bytes)
            .cmp(&(a.original_bytes - a.optimized_bytes))
            .then_with(|| a.path.cmp(&b.path))
    });
    write_new(
        &directory.join("plan.json"),
        &serde_json::to_vec_pretty(&plan)?,
    )?;
    Ok(plan)
}

fn save_blob(directory: &Path, folder: &str, digest: &str, bytes: &[u8]) -> Result<()> {
    let path = directory.join(folder).join(format!("{digest}.png"));
    if path.exists() {
        read_verified(&path, digest)?;
    } else {
        write_new(&path, bytes)?;
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    fs::File::open(path)?
        .take(optimizer::MAX_INPUT as u64 + 1)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() <= optimizer::MAX_INPUT,
        "input exceeds 64 MiB limit"
    );
    Ok(data)
}

pub fn read_plan(directory: impl AsRef<Path>) -> Result<Plan> {
    let directory = fs::canonicalize(directory)?;
    let path = contained_file(&directory, Path::new("plan.json"))?;
    let plan: Plan = serde_json::from_slice(&read_bounded(&path)?)?;
    ensure!(plan.schema_version == 1, "unsupported plan schema version");
    ensure!(
        plan.backend == "oxipng/10.2.1; strict-png/1",
        "unsupported optimization backend"
    );
    plan.policy.validate()?;
    ensure!(
        plan.root.is_absolute() && fs::canonicalize(&plan.root)? == plan.root,
        "project root moved or changed"
    );
    let mut paths = BTreeSet::new();
    for candidate in &plan.candidates {
        ensure!(paths.insert(&candidate.path), "duplicate candidate path");
        for digest in [
            &candidate.original_sha256,
            &candidate.optimized_sha256,
            &candidate.contents_sha256,
        ] {
            ensure!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
                "invalid SHA-256 digest"
            );
        }
        ensure!(
            candidate.optimized_bytes < candidate.original_bytes
                && candidate.original_bytes <= optimizer::MAX_INPUT as u64,
            "invalid candidate sizes"
        );
    }
    Ok(plan)
}

#[derive(Debug, Serialize)]
pub struct ApplyReport {
    pub schema_version: u32,
    pub changed: usize,
    pub already_current: usize,
    pub source_bytes_saved: u64,
}

impl Default for ApplyReport {
    fn default() -> Self {
        Self {
            schema_version: 1,
            changed: 0,
            already_current: 0,
            source_bytes_saved: 0,
        }
    }
}

/// Apply a reviewed plan; no encoding or implicit approval occurs here.
pub fn apply(directory: impl AsRef<Path>) -> Result<ApplyReport> {
    execute(directory.as_ref(), false)
}

/// Restore only files that still match this plan's original or candidate hashes.
pub fn restore(directory: impl AsRef<Path>) -> Result<ApplyReport> {
    execute(directory.as_ref(), true)
}

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn execute(directory: &Path, restoring: bool) -> Result<ApplyReport> {
    let directory = fs::canonicalize(directory)?;
    let lock_path = directory.join(".lock");
    write_new(
        &lock_path,
        format!("pid={}\n", std::process::id()).as_bytes(),
    )
    .context(
        "plan locked; remove .lock only after confirming no resopt process is using this plan",
    )?;
    let _lock = Lock(lock_path);
    let plan = read_plan(&directory)?;
    let root_lock_path = plan.root.join(".resopt.lock");
    write_new(&root_lock_path, format!("pid={}\n", std::process::id()).as_bytes())
        .context("project locked; remove .resopt.lock only after confirming no resopt process is modifying this project")?;
    let _root_lock = Lock(root_lock_path);
    let inventory = scan_with_options(
        &plan.root,
        crate::ScanOptions {
            include_ignored: true,
        },
    )?;
    let assets: BTreeMap<_, _> = inventory
        .assets
        .into_iter()
        .map(|asset| (asset.path.clone(), asset))
        .collect();
    // Full preflight prevents a stale later entry from producing a partial batch.
    for candidate in &plan.candidates {
        let asset = assets
            .get(&candidate.path)
            .context("candidate is no longer referenced by a supported catalog")?;
        ensure!(
            asset.eligible
                && asset.contents_path == candidate.contents_path
                && asset.contents_sha256 == candidate.contents_sha256,
            "catalog eligibility or Contents.json changed: {}",
            candidate.path.display()
        );
        verify_entry(&plan, &directory, candidate)?;
    }
    let journal_path = directory.join("journal.jsonl");
    if fs::symlink_metadata(&journal_path).is_ok() {
        contained_file(&directory, Path::new("journal.jsonl"))?;
    }
    let mut journal = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(journal_path)?;
    let mut report = ApplyReport::default();
    for candidate in &plan.candidates {
        // Recheck right before replacement, in addition to the batch preflight.
        let (source, original, optimized, current) = verify_entry(&plan, &directory, candidate)?;
        let (target, expected_hash) = if restoring {
            (&original, &candidate.original_sha256)
        } else {
            (&optimized, &candidate.optimized_sha256)
        };
        if current == *expected_hash {
            report.already_current += 1;
            continue;
        }
        let operation = if restoring { "restore" } else { "apply" };
        event(&mut journal, operation, "started", &candidate.path)?;
        replace(&source, target).with_context(|| {
            format!(
                "{operation} failed; originals remain in {}; run resopt restore to recover",
                directory.display()
            )
        })?;
        read_verified(&source, expected_hash)?;
        event(&mut journal, operation, "completed", &candidate.path)?;
        report.changed += 1;
        if !restoring {
            report.source_bytes_saved += candidate.original_bytes - candidate.optimized_bytes;
        }
    }
    Ok(report)
}

type VerifiedEntry = (PathBuf, Vec<u8>, Vec<u8>, String);

fn verify_entry(plan: &Plan, directory: &Path, candidate: &Candidate) -> Result<VerifiedEntry> {
    let source = contained_file(&plan.root, &candidate.path)?;
    read_verified(
        &contained_file(&plan.root, &candidate.contents_path)?,
        &candidate.contents_sha256,
    )?;
    let original = blob(directory, "originals", &candidate.original_sha256)?;
    let optimized = blob(directory, "candidates", &candidate.optimized_sha256)?;
    ensure!(
        original.len() as u64 == candidate.original_bytes
            && optimized.len() as u64 == candidate.optimized_bytes,
        "candidate sizes do not match blobs"
    );
    optimizer::verify(&original, &optimized, plan.policy.reductions)?;
    let current = hash(&read_bounded(&source)?);
    ensure!(
        current == candidate.original_sha256 || current == candidate.optimized_sha256,
        "source changed since plan: {}",
        candidate.path.display()
    );
    Ok((source, original, optimized, current))
}

fn blob(directory: &Path, folder: &str, digest: &str) -> Result<Vec<u8>> {
    let path = contained_file(directory, &Path::new(folder).join(format!("{digest}.png")))?;
    let data = read_bounded(&path)?;
    ensure!(
        hash(&data) == digest,
        "artifact hash mismatch: {}",
        path.display()
    );
    Ok(data)
}

fn event(file: &mut fs::File, operation: &str, status: &str, path: &Path) -> Result<()> {
    serde_json::to_writer(
        &mut *file,
        &serde_json::json!({"operation":operation,"status":status,"path":path}),
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
