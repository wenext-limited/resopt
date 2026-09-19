//! Persistent, content-addressed cache of per-image analysis results.
//!
//! An entry is reused only when the source bytes, candidate policy, analysis
//! options, tool version and codec backend all match. Entries are written to a
//! temporary directory and renamed into place, every file is hash-checked on
//! read, and anything unexpected discards the entry instead of being trusted.
use crate::{
    AnalysisOptions, ResourceAnalysis,
    filesystem::{hash, write_new},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::SystemTime,
};

/// Bump when the entry layout or any cached measurement changes meaning.
const CACHE_SCHEMA: u32 = 4;
const MAX_ENTRY_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Default size bound; the oldest entries are pruned after each analysis.
pub(crate) const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(crate) struct Cache {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    schema: u32,
    key: String,
    analysis: ResourceAnalysis,
    files: Vec<EntryFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryFile {
    /// Report-relative path with the resource index replaced by `{i}`.
    template: String,
    name: String,
    sha256: String,
}

/// The options that influence candidate bytes or verdicts. Presentation and
/// scheduling options (jobs, cache settings, ignore rules) are excluded.
#[derive(Serialize)]
struct KeyMaterial<'a> {
    schema: u32,
    version: &'a str,
    backend: &'a str,
    source_sha256: &'a str,
    policy: &'a str,
    qualities: &'a [u8],
    min_input_bytes: u64,
    min_savings_bytes: u64,
    max_alpha_error: u32,
    min_score: u64,
    max_pixels: usize,
    png_level: u8,
    png_reductions: bool,
    webp: bool,
}

/// Codec identity. ImageIO output can change between macOS builds; the bundled
/// codecs only change with the tool version, which is keyed separately.
pub(crate) fn backend_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        if cfg!(target_os = "macos") {
            let build = std::process::Command::new("/usr/bin/sw_vers")
                .arg("-buildVersion")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.len() <= 32)
                .unwrap_or_else(|| "unknown".into());
            format!("macos-imageio-{build}")
        } else {
            format!("portable-{}", std::env::consts::OS)
        }
    })
}

/// Per-user cache location; `RESOPT_CACHE_DIR` overrides it.
pub fn default_directory() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(path) = var("RESOPT_CACHE_DIR") {
        return Some(PathBuf::from(path));
    }
    if cfg!(target_os = "macos") {
        var("HOME").map(|home| PathBuf::from(home).join("Library/Caches/resopt"))
    } else if cfg!(windows) {
        var("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("resopt").join("cache"))
    } else {
        var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .map(|dir| dir.join("resopt"))
    }
}

impl Cache {
    pub fn open(directory: &Path) -> Result<Self> {
        let root = directory.join(format!("v{CACHE_SCHEMA}"));
        fs::create_dir_all(&root)
            .with_context(|| format!("creating cache directory {}", root.display()))?;
        let root = fs::canonicalize(root)?;
        Ok(Self { root })
    }

    pub fn key(source_sha256: &str, policy: &str, options: &AnalysisOptions) -> Result<String> {
        Ok(hash(&serde_json::to_vec(&KeyMaterial {
            schema: CACHE_SCHEMA,
            version: env!("CARGO_PKG_VERSION"),
            backend: backend_id(),
            source_sha256,
            policy,
            qualities: &options.qualities,
            min_input_bytes: options.min_input_bytes,
            min_savings_bytes: options.min_savings_bytes,
            max_alpha_error: options.max_alpha_error.to_bits(),
            min_score: options.min_score.to_bits(),
            max_pixels: options.max_pixels,
            png_level: options.png_level,
            png_reductions: options.png_reductions,
            webp: options.webp,
        })?))
    }

    fn entry_directory(&self, key: &str) -> Result<PathBuf> {
        ensure!(
            key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid cache key"
        );
        Ok(self.root.join(&key[..2]).join(key))
    }

    /// Copy a verified entry into `out`, renaming files for `index`. Any
    /// inconsistency removes the entry and reports a miss.
    pub fn load(&self, key: &str, out: &Path, index: usize) -> Option<ResourceAnalysis> {
        let directory = self.entry_directory(key).ok()?;
        if !directory.is_dir() {
            return None;
        }
        match self.load_verified(&directory, key, out, index) {
            Ok(analysis) => {
                // Recency drives pruning.
                let _ = fs::File::options()
                    .write(true)
                    .open(directory.join("entry.json"))
                    .and_then(|file| file.set_modified(SystemTime::now()));
                Some(analysis)
            }
            Err(_) => {
                let _ = fs::remove_dir_all(&directory);
                None
            }
        }
    }

    fn load_verified(
        &self,
        directory: &Path,
        key: &str,
        out: &Path,
        index: usize,
    ) -> Result<ResourceAnalysis> {
        let manifest = directory.join("entry.json");
        ensure!(
            fs::symlink_metadata(&manifest)?.len() <= MAX_ENTRY_FILE_BYTES,
            "cache manifest too large"
        );
        let entry: Entry = serde_json::from_slice(&fs::read(&manifest)?)?;
        ensure!(
            entry.schema == CACHE_SCHEMA && entry.key == key,
            "cache entry mismatch"
        );
        let mut staged = Vec::with_capacity(entry.files.len());
        for file in &entry.files {
            ensure!(
                file.name.len() <= 8
                    && file.name.bytes().all(|b| b.is_ascii_alphanumeric())
                    && safe_template(&file.template),
                "unsafe cache file"
            );
            let path = directory.join(&file.name);
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.file_type().is_file() && metadata.len() <= MAX_ENTRY_FILE_BYTES,
                "invalid cache file"
            );
            let bytes = fs::read(&path)?;
            ensure!(hash(&bytes) == file.sha256, "cache file corrupted");
            staged.push((file.template.replace("{i}", &index.to_string()), bytes));
        }
        let mut analysis = entry.analysis;
        let rename = |path: &mut Option<PathBuf>| -> Result<()> {
            if let Some(template) = path.as_ref().and_then(|p| p.to_str()) {
                ensure!(
                    entry.files.iter().any(|f| f.template == template),
                    "cache entry references a missing file"
                );
                *path = Some(PathBuf::from(template.replace("{i}", &index.to_string())));
            }
            Ok(())
        };
        rename(&mut analysis.original_preview)?;
        for candidate in &mut analysis.candidates {
            rename(&mut candidate.artifact)?;
            rename(&mut candidate.preview)?;
        }
        let mut written = Vec::with_capacity(staged.len());
        for (relative, bytes) in staged {
            let path = out.join(relative);
            if let Err(error) = crate::filesystem::write_artifact(&path, &bytes) {
                // Leave no partial copy behind: the caller re-analyzes into the
                // same file names.
                for path in written {
                    let _ = fs::remove_file(path);
                }
                return Err(error);
            }
            written.push(path);
        }
        Ok(analysis)
    }

    /// Store a completed analysis. `analysis` paths must be the report-relative
    /// names written for `index`; the original artifact is never cached because
    /// it is a copy of the source.
    pub fn store(
        &self,
        key: &str,
        analysis: &ResourceAnalysis,
        out: &Path,
        index: usize,
    ) -> Result<()> {
        let directory = self.entry_directory(key)?;
        if directory.exists() {
            return Ok(());
        }
        let parent = directory.parent().context("cache entry has no parent")?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix("tmp-")
            .tempdir_in(&self.root)?;
        let mut template_analysis = analysis.clone();
        template_analysis.original_artifact = None;
        let mut files = Vec::new();
        let mut add = |path: &mut Option<PathBuf>| -> Result<()> {
            let Some(relative) = path.clone() else {
                return Ok(());
            };
            let template = template_for(&relative, index)?;
            let bytes = fs::read(out.join(&relative))?;
            let name = format!("f{}", files.len());
            // No fsync: a torn file fails its hash check and discards the entry.
            crate::filesystem::write_artifact(&staging.path().join(&name), &bytes)?;
            files.push(EntryFile {
                template: template.clone(),
                name,
                sha256: hash(&bytes),
            });
            *path = Some(PathBuf::from(template));
            Ok(())
        };
        add(&mut template_analysis.original_preview)?;
        for candidate in &mut template_analysis.candidates {
            add(&mut candidate.artifact)?;
            add(&mut candidate.preview)?;
        }
        let entry = Entry {
            schema: CACHE_SCHEMA,
            key: key.to_string(),
            analysis: template_analysis,
            files,
        };
        // The manifest is written last, so a directory without it is never used.
        write_new(
            &staging.path().join("entry.json"),
            &serde_json::to_vec(&entry)?,
        )?;
        let staged = staging.keep();
        if fs::rename(&staged, &directory).is_err() {
            // Another process stored the same key first; its entry is equivalent.
            let _ = fs::remove_dir_all(&staged);
        }
        Ok(())
    }

    /// Remove abandoned staging directories and the oldest entries above `max_bytes`.
    pub fn prune(&self, max_bytes: u64) -> Result<()> {
        let mut entries = Vec::new();
        let mut total = 0_u64;
        for shard in fs::read_dir(&self.root)? {
            let shard = shard?.path();
            let name = shard.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with("tmp-") {
                let stale = fs::metadata(&shard)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age.as_secs() > 3600);
                if stale {
                    let _ = fs::remove_dir_all(&shard);
                }
                continue;
            }
            if !shard.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&shard)? {
                let entry = entry?.path();
                let Ok(manifest) = fs::metadata(entry.join("entry.json")) else {
                    let _ = fs::remove_dir_all(&entry);
                    continue;
                };
                let bytes: u64 = fs::read_dir(&entry)?
                    .filter_map(|f| f.ok()?.metadata().ok())
                    .map(|m| m.len())
                    .sum();
                total += bytes;
                entries.push((
                    manifest.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    bytes,
                    entry,
                ));
            }
        }
        entries.sort();
        for (_, bytes, entry) in entries {
            if total <= max_bytes {
                break;
            }
            if fs::remove_dir_all(&entry).is_ok() {
                total = total.saturating_sub(bytes);
            }
        }
        Ok(())
    }
}

fn safe_template(template: &str) -> bool {
    let mut parts = template.split('/');
    matches!(parts.next(), Some("candidates" | "previews"))
        && parts.next().is_some_and(|name| {
            name.starts_with("{i}-")
                && name[3..]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
                && !name.contains("..")
        })
        && parts.next().is_none()
}

fn template_for(relative: &Path, index: usize) -> Result<String> {
    let text = relative
        .to_str()
        .context("non-UTF8 artifact path")?
        .replace('\\', "/");
    let (folder, name) = text
        .split_once('/')
        .context("artifact path has no folder")?;
    let rest = name
        .strip_prefix(&format!("{index}-"))
        .context("artifact name does not start with its resource index")?;
    let template = format!("{folder}/{{i}}-{rest}");
    ensure!(safe_template(&template), "unsupported artifact name {text}");
    Ok(template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageCandidate, Resource};

    fn analysis(index: usize) -> ResourceAnalysis {
        ResourceAnalysis {
            resource: Resource::for_tests("a/picture.png", "png"),
            sha256: Some("0".repeat(64)),
            image: None,
            status: "candidates_available".into(),
            issues: vec![],
            candidates: vec![ImageCandidate {
                format: "png".into(),
                quality: None,
                lossy: false,
                bytes: 3,
                savings_bytes: 5,
                valid: true,
                rejection: None,
                difference: None,
                artifact: Some(format!("candidates/{index}-png-0.png").into()),
                preview: Some(format!("previews/{index}-png-0.png").into()),
                sha256: Some(hash(b"art")),
                warnings: vec![],
                notes: vec![],
            }],
            smallest_candidate: Some(0),
            original_preview: Some(format!("previews/{index}-original.png").into()),
            original_artifact: Some(format!("originals/{index}.png").into()),
            fingerprint: None,
            media: None,
            animation: None,
        }
    }

    fn report_directory(index: usize) -> tempfile::TempDir {
        let out = tempfile::tempdir().unwrap();
        for folder in ["candidates", "previews", "originals"] {
            fs::create_dir(out.path().join(folder)).unwrap();
        }
        fs::write(
            out.path().join(format!("candidates/{index}-png-0.png")),
            b"art",
        )
        .unwrap();
        fs::write(
            out.path().join(format!("previews/{index}-png-0.png")),
            b"pre",
        )
        .unwrap();
        fs::write(
            out.path().join(format!("previews/{index}-original.png")),
            b"orig-preview",
        )
        .unwrap();
        out
    }

    fn empty_report() -> tempfile::TempDir {
        let out = tempfile::tempdir().unwrap();
        for folder in ["candidates", "previews", "originals"] {
            fs::create_dir(out.path().join(folder)).unwrap();
        }
        out
    }

    #[test]
    fn round_trip_renames_files_for_the_new_resource_index() {
        let home = tempfile::tempdir().unwrap();
        let cache = Cache::open(home.path()).unwrap();
        let key = Cache::key(&"1".repeat(64), "loose", &AnalysisOptions::default()).unwrap();
        let first = report_directory(4);
        cache.store(&key, &analysis(4), first.path(), 4).unwrap();
        let second = empty_report();
        let loaded = cache.load(&key, second.path(), 9).unwrap();
        assert_eq!(
            loaded.candidates[0].artifact.as_deref(),
            Some(Path::new("candidates/9-png-0.png"))
        );
        assert_eq!(loaded.original_artifact, None);
        assert_eq!(
            fs::read(second.path().join("candidates/9-png-0.png")).unwrap(),
            b"art"
        );
        assert_eq!(
            fs::read(second.path().join("previews/9-original.png")).unwrap(),
            b"orig-preview"
        );
    }

    #[test]
    fn key_changes_with_source_policy_and_relevant_options_only() {
        let options = AnalysisOptions::default();
        let base = Cache::key(&"1".repeat(64), "loose", &options).unwrap();
        assert_ne!(
            base,
            Cache::key(&"2".repeat(64), "loose", &options).unwrap()
        );
        assert_ne!(
            base,
            Cache::key(&"1".repeat(64), "catalog", &options).unwrap()
        );
        for changed in [
            AnalysisOptions {
                qualities: vec![85],
                ..options.clone()
            },
            AnalysisOptions {
                webp: !options.webp,
                ..options.clone()
            },
            AnalysisOptions {
                min_score: 50.0,
                ..options.clone()
            },
            AnalysisOptions {
                png_reductions: true,
                ..options.clone()
            },
        ] {
            assert_ne!(
                base,
                Cache::key(&"1".repeat(64), "loose", &changed).unwrap()
            );
        }
        let scheduling_only = AnalysisOptions {
            jobs: 1,
            include_ignored: true,
            ..options.clone()
        };
        assert_eq!(
            base,
            Cache::key(&"1".repeat(64), "loose", &scheduling_only).unwrap()
        );
    }

    #[test]
    fn corrupted_truncated_and_interrupted_entries_are_discarded() {
        let home = tempfile::tempdir().unwrap();
        let cache = Cache::open(home.path()).unwrap();
        let key = Cache::key(&"1".repeat(64), "loose", &AnalysisOptions::default()).unwrap();
        let source = report_directory(0);
        let directory = cache.entry_directory(&key).unwrap();

        // Corrupted payload.
        cache.store(&key, &analysis(0), source.path(), 0).unwrap();
        fs::write(directory.join("f1"), b"tampered").unwrap();
        let out = empty_report();
        assert!(cache.load(&key, out.path(), 0).is_none());
        assert!(!directory.exists());
        assert!(
            fs::read_dir(out.path().join("candidates"))
                .unwrap()
                .next()
                .is_none()
        );

        // Interrupted write: files without a manifest.
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("f0"), b"art").unwrap();
        assert!(cache.load(&key, empty_report().path(), 0).is_none());
        assert!(!directory.exists());

        // Truncated manifest and a manifest for another key.
        cache.store(&key, &analysis(0), source.path(), 0).unwrap();
        fs::write(directory.join("entry.json"), b"{\"schema\":1").unwrap();
        assert!(cache.load(&key, empty_report().path(), 0).is_none());
        cache.store(&key, &analysis(0), source.path(), 0).unwrap();
        let other = Cache::key(&"3".repeat(64), "loose", &AnalysisOptions::default()).unwrap();
        let other_directory = cache.entry_directory(&other).unwrap();
        fs::create_dir_all(other_directory.parent().unwrap()).unwrap();
        fs::rename(&directory, &other_directory).unwrap();
        assert!(cache.load(&other, empty_report().path(), 0).is_none());
    }

    #[test]
    fn traversal_templates_are_never_written() {
        assert!(safe_template("candidates/{i}-png-0.png"));
        for template in [
            "candidates/../{i}-x.png",
            "../{i}-x.png",
            "originals/{i}-x.png",
            "candidates/{i}-../x",
            "candidates/x.png",
            "candidates/{i}-a/b.png",
        ] {
            assert!(!safe_template(template), "{template}");
        }
    }

    #[test]
    fn prune_removes_oldest_entries_and_stale_staging() {
        let home = tempfile::tempdir().unwrap();
        let cache = Cache::open(home.path()).unwrap();
        let source = report_directory(0);
        let keys: Vec<_> = (0..3)
            .map(|i| {
                Cache::key(
                    &i.to_string().repeat(64),
                    "loose",
                    &AnalysisOptions::default(),
                )
                .unwrap()
            })
            .collect();
        for (age, key) in keys.iter().enumerate() {
            cache.store(key, &analysis(0), source.path(), 0).unwrap();
            let manifest = cache.entry_directory(key).unwrap().join("entry.json");
            fs::File::options()
                .write(true)
                .open(manifest)
                .unwrap()
                .set_modified(
                    SystemTime::now() - std::time::Duration::from_secs(1000 - age as u64 * 100),
                )
                .unwrap();
        }
        let entry_bytes: u64 = fs::read_dir(cache.entry_directory(&keys[0]).unwrap())
            .unwrap()
            .map(|f| f.unwrap().metadata().unwrap().len())
            .sum();
        cache.prune(entry_bytes * 2).unwrap();
        assert!(!cache.entry_directory(&keys[0]).unwrap().exists());
        assert!(cache.entry_directory(&keys[1]).unwrap().exists());
        assert!(cache.entry_directory(&keys[2]).unwrap().exists());
    }
}
