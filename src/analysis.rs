use crate::{
    ImageDifference, ImageInfo, Policy, Resource, ResourceInventory,
    analyze_image::{self, Context as ImageContext},
    cache::Cache,
    filesystem::{contained_file, hash, write_new},
    image_backend,
    resources::{bounded_read, inventory_with_options},
    timings::{Phase, Timings},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

/// Candidate verdicts a user may accept after reviewing the actual result.
/// Every other rejection is a hard failure that approval cannot bypass.
pub(crate) const WARNINGS: [&str; 3] = [
    "alpha_error_exceeds_policy",
    "transparency_presence_changed",
    "quality_below_policy",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnalysisOptions {
    /// Encoder quality parameters for lossy candidates; not savings percentages.
    pub qualities: Vec<u8>,
    pub include_ignored: bool,
    /// Parallel image workers; 0 selects the CPU count, capped at 8.
    pub jobs: usize,
    /// All sizes are included by default, unlike the legacy lossless plan.
    pub min_input_bytes: u64,
    pub min_savings_bytes: u64,
    pub probe_only: bool,
    /// Lossy HEIC may quantize alpha. 0 requires exact alpha samples.
    pub max_alpha_error: f32,
    /// Lowest SSIMULACRA2 score a lossy candidate may have and still be
    /// recommended. Lower-scoring candidates are kept as reviewable warnings.
    pub min_score: f64,
    /// Largest decoded image analyzed; each pixel costs 16 bytes per decode.
    pub max_pixels: usize,
    /// oxipng effort for the lossless PNG candidate.
    pub png_level: u8,
    /// Allow lossless PNG color-type, bit-depth and palette reductions.
    pub png_reductions: bool,
    /// Compare WebP candidates for loose files and Android resources. On by
    /// default; asset-catalog renditions never receive WebP.
    pub webp: bool,
    /// Compare lossy PNG candidates (palette quantization) for PNG sources.
    /// The file stays a PNG, so names, references and decoders are unaffected.
    pub lossy_png: bool,
    /// Also try HEIC at encoder quality 100. Apple's encoder has no lossless
    /// mode, so this is its closest setting: still lossy, and labelled so.
    pub heic_near_lossless: bool,
    /// Overrides the `minSdk` detected from Gradle files.
    pub android_min_sdk: Option<u32>,
    /// Persistent result cache directory. `None` disables the cache; the CLI
    /// passes the per-user cache directory unless `--no-cache` is given.
    pub cache_dir: Option<PathBuf>,
}
impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            qualities: vec![75, 85, 95],
            include_ignored: false,
            jobs: 0,
            min_input_bytes: 0,
            min_savings_bytes: 1,
            probe_only: false,
            max_alpha_error: 1.0 / 255.0 + 0.000001,
            min_score: 80.0,
            max_pixels: image_backend::DEFAULT_MAX_PIXELS,
            png_level: Policy::default().png_level,
            png_reductions: false,
            webp: true,
            lossy_png: true,
            heic_near_lossless: true,
            android_min_sdk: None,
            cache_dir: None,
        }
    }
}
impl AnalysisOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.max_alpha_error.is_finite() && (0.0..=1.0).contains(&self.max_alpha_error),
            "max_alpha_error must be 0..=1"
        );
        ensure!(
            self.min_score.is_finite() && (0.0..=100.0).contains(&self.min_score),
            "min_score must be 0..=100"
        );
        ensure!(
            self.jobs <= 16,
            "jobs must be 0..=16 (0 selects automatically)"
        );
        ensure!(
            self.android_min_sdk.is_none_or(|v| (1..=99).contains(&v)),
            "android_min_sdk must be 1..=99"
        );
        ensure!(
            (1..=image_backend::MAX_PIXELS_LIMIT).contains(&self.max_pixels),
            "max_pixels must be 1..={}",
            image_backend::MAX_PIXELS_LIMIT
        );
        self.png_policy().validate()?;
        ensure!(
            !self.qualities.is_empty()
                && self.qualities.len() <= 8
                && self.qualities.iter().all(|q| (1..=100).contains(q)),
            "qualities must contain 1..=8 values in 1..=100"
        );
        let mut qualities = self.qualities.clone();
        qualities.sort_unstable();
        qualities.dedup();
        ensure!(
            qualities.len() == self.qualities.len(),
            "duplicate quality values"
        );
        Ok(())
    }

    pub(crate) fn png_policy(&self) -> Policy {
        Policy {
            png_level: self.png_level,
            reductions: self.png_reductions,
            ..Policy::default()
        }
    }

    pub(crate) fn worker_count(&self) -> usize {
        if self.jobs > 0 {
            return self.jobs;
        }
        std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 8)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCandidate {
    pub format: String,
    pub quality: Option<u8>,
    pub lossy: bool,
    pub bytes: u64,
    pub savings_bytes: u64,
    pub valid: bool,
    pub rejection: Option<String>,
    pub difference: Option<ImageDifference>,
    pub artifact: Option<PathBuf>,
    pub preview: Option<PathBuf>,
    /// SHA-256 of the artifact, checked again before it is applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Every visual policy threshold this candidate misses. `rejection` holds
    /// the first one; approval must cover all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Facts the reviewer should know, e.g. metadata a conversion does not carry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl ImageCandidate {
    /// A structurally sound candidate that only misses a visual policy threshold.
    pub(crate) fn is_warning(&self) -> bool {
        !self.valid
            && self.lossy
            && self.artifact.is_some()
            && self
                .rejection
                .as_deref()
                .is_some_and(|r| WARNINGS.contains(&r))
    }

    /// Warning kinds that must be approved before this candidate is applied.
    pub(crate) fn required_warnings(&self) -> Vec<String> {
        if !self.is_warning() {
            vec![]
        } else if self.warnings.is_empty() {
            // Reports written before `warnings` existed recorded one kind.
            self.rejection.iter().cloned().collect()
        } else {
            self.warnings.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAnalysis {
    pub resource: Resource,
    pub sha256: Option<String>,
    pub image: Option<ImageInfo>,
    /// `candidates_available`, `inspected`, `excluded`, `unsupported`, `failed`
    /// or `not_analyzed` (analysis was cancelled first).
    pub status: String,
    pub issues: Vec<String>,
    pub candidates: Vec<ImageCandidate>,
    /// A size winner among candidates that passed every policy check.
    pub smallest_candidate: Option<usize>,
    pub original_preview: Option<PathBuf>,
    pub original_artifact: Option<PathBuf>,
    /// Codec, duration and bitrate of audio/video files, when ffprobe is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<crate::media::MediaInfo>,
    /// Bounded ZIP contents; entries are never extracted into the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<crate::archive::ArchiveInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vap: Option<crate::vap::VapInfo>,
    /// Canvas, timing and size of an animation, when it could be parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationInfo>,
    /// Scale-invariant fingerprint used to find duplicate and resized images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<crate::similarity::Fingerprint>,
}

impl ResourceAnalysis {
    pub(crate) fn new(resource: &Resource, status: &str) -> Self {
        Self {
            resource: resource.clone(),
            sha256: None,
            image: None,
            status: status.into(),
            issues: vec![],
            candidates: vec![],
            smallest_candidate: None,
            original_preview: None,
            original_artifact: None,
            fingerprint: None,
            media: None,
            archive: None,
            vap: None,
            animation: None,
        }
    }

    pub(crate) fn recommended_savings(&self) -> u64 {
        self.smallest_candidate
            .and_then(|i| self.candidates.get(i))
            .filter(|c| c.valid && c.artifact.is_some())
            .map_or(0, |c| c.savings_bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationInfo {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: usize,
    /// Frame shown as the thumbnail.
    pub poster_frame: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub root: PathBuf,
    pub backend: String,
    pub options: AnalysisOptions,
    pub inventory: ResourceInventory,
    pub resources: Vec<ResourceAnalysis>,
    pub status_counts: BTreeMap<String, usize>,
    /// Sum of recommended candidates only; warning candidates are excluded.
    pub potential_source_bytes_saved: u64,
    /// Images that show the same picture (identical, resized or near-duplicate),
    /// excluding intended variants such as `@2x`/`@3x` or density folders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub similar_groups: Vec<crate::similarity::SimilarGroup>,
    /// Analysis stopped early; unfinished resources are `not_analyzed`.
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<Performance>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Performance {
    pub wall_seconds: f64,
    pub first_result_seconds: Option<f64>,
    pub workers: usize,
    pub cache_hits: usize,
    pub duplicate_reuses: usize,
    /// Seconds per phase summed over workers (CPU-side, not wall-clock).
    pub phase_seconds: BTreeMap<String, f64>,
}

/// Cooperative cancellation shared with the caller.
#[derive(Clone, Default)]
pub struct AnalysisControl(Arc<AtomicBool>);
impl AnalysisControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Bounds decoded source pixels in flight to one maximum-size image, so adding
/// workers speeds up ordinary assets without multiplying peak memory.
pub(crate) struct PixelBudget {
    capacity: usize,
    available: Mutex<usize>,
    released: Condvar,
}
pub(crate) struct PixelLease<'a>(&'a PixelBudget, usize);
impl PixelBudget {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            available: Mutex::new(capacity),
            released: Condvar::new(),
        }
    }
    pub fn acquire(&self, pixels: usize) -> PixelLease<'_> {
        let wanted = pixels.clamp(1, self.capacity);
        let mut available = self.available.lock().unwrap_or_else(|e| e.into_inner());
        while *available < wanted {
            available = self
                .released
                .wait(available)
                .unwrap_or_else(|e| e.into_inner());
        }
        *available -= wanted;
        PixelLease(self, wanted)
    }
}
impl Drop for PixelLease<'_> {
    fn drop(&mut self) {
        *self.0.available.lock().unwrap_or_else(|e| e.into_inner()) += self.1;
        self.0.released.notify_all();
    }
}

/// Read-only analysis of all inventoried resources, with lossy candidates staged
/// solely for review. This report is deliberately not an executable apply plan.
pub fn analyze(
    root: impl AsRef<Path>,
    out: impl AsRef<Path>,
    options: AnalysisOptions,
) -> Result<AnalysisReport> {
    analyze_with_progress(root, out, options, |_, _| {})
}

pub fn analyze_with_progress(
    root: impl AsRef<Path>,
    out: impl AsRef<Path>,
    options: AnalysisOptions,
    progress: impl Fn(usize, usize) + Sync,
) -> Result<AnalysisReport> {
    analyze_with_observer(
        root,
        out,
        options,
        &AnalysisControl::default(),
        |_, _, done, total| progress(done, total),
    )
}

type Shared = Arc<OnceLock<(usize, ResourceAnalysis)>>;

pub(crate) fn analyze_with_observer(
    root: impl AsRef<Path>,
    out: impl AsRef<Path>,
    options: AnalysisOptions,
    control: &AnalysisControl,
    progress: impl Fn(usize, &ResourceAnalysis, usize, usize) + Sync,
) -> Result<AnalysisReport> {
    let started = Instant::now();
    options.validate()?;
    if !options.probe_only && image_backend::image_backend_available() {
        image_backend::check_encoders()?;
    }
    let timings = Timings::default();
    let inventory = timings.time(Phase::Scan, || {
        inventory_with_options(
            root,
            crate::ScanOptions {
                include_ignored: options.include_ignored,
            },
        )
    })?;
    let out = out.as_ref();
    let parent = fs::canonicalize(
        out.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let out = parent.join(out.file_name().context("output directory has no name")?);
    ensure!(
        !out.starts_with(&inventory.root),
        "analysis output must be outside the scanned project"
    );
    fs::create_dir(&out).context("analysis output must be a new directory")?;
    for folder in ["candidates", "previews", "originals"] {
        fs::create_dir(out.join(folder))?;
    }
    let cache = options
        .cache_dir
        .as_ref()
        .filter(|_| !options.probe_only)
        .and_then(|directory| Cache::open(directory).ok());
    let min_sdk = options
        .android_min_sdk
        .or(inventory.android_min_sdk.as_ref().map(|sdk| sdk.level));
    let workers = options.worker_count();
    let budget = PixelBudget::new(options.max_pixels);
    let context = ImageContext {
        root: &inventory.root,
        out: &out,
        options: &options,
        min_sdk,
        timings: &timings,
        control,
        budget: &budget,
    };
    let total = inventory.assets.len();
    let complete = AtomicUsize::new(0);
    let cache_hits = AtomicUsize::new(0);
    let duplicate_reuses = AtomicUsize::new(0);
    let first_result = OnceLock::new();
    let in_flight: Mutex<HashMap<String, Shared>> = Mutex::new(HashMap::new());
    let finish = |index: usize, result: ResourceAnalysis| {
        if result.status == "candidates_available" {
            first_result.get_or_init(|| started.elapsed().as_secs_f64());
        }
        let done = complete.fetch_add(1, Ordering::Relaxed) + 1;
        progress(index, &result, done, total);
        (index, result)
    };

    // Rows that need no image work are published first so the inventory is
    // visible immediately; images follow largest-first because they hold most
    // of the savings.
    let (mut work, settled): (Vec<usize>, Vec<usize>) = (0..total).partition(|&index| {
        let resource = &inventory.assets[index];
        resource.support == "optimizable"
            || matches!(resource.format.as_str(), "zip" | "mp4" | "vap")
            || (resource.kind == "image" && options.probe_only)
            || (is_media(resource) && crate::media::ffprobe_available())
    });
    work.sort_by_key(|&index| std::cmp::Reverse(inventory.assets[index].bytes));
    let mut indexed: Vec<(usize, ResourceAnalysis)> = settled
        .into_iter()
        .map(|index| finish(index, settled_row(&inventory.assets[index])))
        .collect();

    let analyze_one = |index: usize| -> ResourceAnalysis {
        let resource = &inventory.assets[index];
        if control.is_cancelled() {
            return ResourceAnalysis::new(resource, "not_analyzed");
        }
        if is_media(resource) && resource.format != "mp4" {
            // Inspection only: spawning ffprobe runs on the worker pool so it
            // never delays image results.
            let mut row = settled_row(resource);
            if let Ok(path) = contained_file(&inventory.root, &resource.path) {
                row.media = timings.time(Phase::Decode, || crate::media::probe(&path));
            }
            return row;
        }
        let read = timings.time(Phase::Hash, || {
            contained_file(&inventory.root, &resource.path)
                .and_then(|path| bounded_read(&path))
                .map(|bytes| {
                    let digest = hash(&bytes);
                    (bytes, digest)
                })
        });
        let (bytes, digest) = match read {
            Ok(read) => read,
            Err(error) => {
                let mut failed = ResourceAnalysis::new(resource, "failed");
                failed.issues.push(format!("{error:#}"));
                return failed;
            }
        };
        if matches!(resource.format.as_str(), "mp4" | "vap") {
            let mut row = settled_row(resource);
            row.sha256 = Some(digest);
            match crate::vap::inspect(&bytes) {
                Ok(Some(info)) => {
                    row.issues.clear();
                    row.status = "inspected".into();
                    row.resource.kind = "animation".into();
                    row.resource.format = "vap".into();
                    let artifact = PathBuf::from(format!("originals/{index}.mp4"));
                    match write_new(&out.join(&artifact), &bytes) {
                        Ok(()) => row.original_artifact = Some(artifact),
                        Err(error) => row.issues.push(format!("preview_unavailable: {error:#}")),
                    }
                    row.vap = Some(info);
                }
                Ok(None) => {
                    if let Ok(path) = contained_file(&inventory.root, &resource.path) {
                        row.media = crate::media::probe(&path);
                    }
                }
                Err(error) => {
                    row.status = "failed".into();
                    row.issues.push(format!("{error:#}"));
                }
            }
            return row;
        }
        // Archive previews have their own entry paths; do not reuse image-cache manifests.
        if resource.format == "zip" {
            return crate::archive::analyze(&context, resource, index, &bytes, &digest);
        }
        let compute = |own_index: usize| {
            #[cfg(target_os = "macos")]
            {
                objc2::rc::autoreleasepool(|_| {
                    analyze_image::analyze(&context, resource, own_index, &bytes, &digest)
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                analyze_image::analyze(&context, resource, own_index, &bytes, &digest)
            }
        };
        if options.probe_only {
            return compute(index);
        }
        let key = match Cache::key(
            &digest,
            &analyze_image::policy_key(resource, min_sdk),
            &options,
        ) {
            Ok(key) => key,
            Err(_) => return compute(index),
        };
        // Identical content under an identical policy is analyzed once per run.
        let slot = in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.clone())
            .or_default()
            .clone();
        let mut computed_here = false;
        let (owner, shared) = slot.get_or_init(|| {
            computed_here = true;
            if let Some(cache) = &cache
                && let Some(mut hit) = timings.time(Phase::Cache, || cache.load(&key, &out, index))
                && restore_original_artifact(&mut hit, &out, index, &bytes, resource).is_ok()
            {
                cache_hits.fetch_add(1, Ordering::Relaxed);
                hit.resource = resource.clone();
                return (index, hit);
            }
            let result = compute(index);
            if let Some(cache) = &cache
                && matches!(result.status.as_str(), "candidates_available" | "inspected")
            {
                let _ = timings.time(Phase::Cache, || cache.store(&key, &result, &out, index));
            }
            (index, result)
        });
        if computed_here || *owner == index {
            return shared.clone();
        }
        if matches!(shared.status.as_str(), "failed" | "not_analyzed") {
            return compute(index);
        }
        duplicate_reuses.fetch_add(1, Ordering::Relaxed);
        let mut reused = shared.clone();
        reused.resource = resource.clone();
        reused
    };
    // Plain threads pulling from a shared queue, not a rayon pool: oxipng uses
    // rayon internally, and a rayon worker that waits on nested work runs other
    // queued tasks on the same stack. With a task already holding a pixel lease
    // or initializing a shared duplicate slot, that re-entrancy deadlocked.
    let next = AtomicUsize::new(0);
    let finished = Mutex::new(Vec::with_capacity(work.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers.min(work.len()).max(1) {
            scope.spawn(|| {
                while let Some(&index) = work.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let done = finish(index, analyze_one(index));
                    finished
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(done);
                }
            });
        }
    });
    indexed.extend(finished.into_inner().unwrap_or_else(|e| e.into_inner()));
    indexed.sort_by_key(|(index, _)| *index);
    let resources: Vec<_> = indexed.into_iter().map(|(_, result)| result).collect();
    let mut status_counts = BTreeMap::new();
    let mut savings = 0;
    for resource in &resources {
        *status_counts.entry(resource.status.clone()).or_insert(0) += 1;
        savings += resource.recommended_savings();
    }
    if let Some(cache) = &cache {
        let _ = cache.prune(crate::cache::DEFAULT_MAX_BYTES);
    }
    let similar_groups = crate::similarity::group(&resources);
    // Fingerprints exist for grouping (and the cache); the report keeps the groups.
    let resources: Vec<_> = resources
        .into_iter()
        .map(|resource| ResourceAnalysis {
            fingerprint: None,
            ..resource
        })
        .collect();
    let mut report = AnalysisReport {
        schema_version: 2,
        root: inventory.root.clone(),
        backend: if image_backend::image_backend_available() {
            "Apple ImageIO + CoreGraphics sRGB float comparison; bundled oxipng and libwebp"
        } else {
            "Portable PNG and WebP (bundled oxipng and libwebp); JPEG/HEIC require macOS"
        }
        .into(),
        options,
        inventory,
        resources,
        status_counts,
        potential_source_bytes_saved: savings,
        similar_groups,
        cancelled: control.is_cancelled(),
        performance: None,
    };
    let html = timings.time(Phase::Report, || crate::report::render_html(&report))?;
    report.performance = Some(Performance {
        wall_seconds: started.elapsed().as_secs_f64(),
        first_result_seconds: first_result.get().copied(),
        workers,
        cache_hits: cache_hits.load(Ordering::Relaxed),
        duplicate_reuses: duplicate_reuses.load(Ordering::Relaxed),
        phase_seconds: timings.snapshot(),
    });
    write_new(&out.join("analysis.json"), &serde_json::to_vec(&report)?)?;
    write_new(&out.join("report.html"), html.as_bytes())?;
    Ok(report)
}

fn is_media(resource: &Resource) -> bool {
    matches!(resource.kind.as_str(), "audio" | "video") && resource.conversion_exclusion.is_none()
}

/// Inventory rows that need no decoding.
fn settled_row(resource: &Resource) -> ResourceAnalysis {
    if let Some(reason) = &resource.conversion_exclusion {
        let mut row = ResourceAnalysis::new(resource, "excluded");
        row.issues.push(reason.clone());
        return row;
    }
    let mut row = ResourceAnalysis::new(resource, "unsupported");
    if is_media(resource) && !crate::media::ffprobe_available() {
        row.issues.push("ffprobe_not_installed".into());
    }
    row.issues.push(if resource.kind == "image" {
        format!("{}_decoding_requires_macos_imageio", resource.format)
    } else {
        format!("{}_optimization_backend_not_implemented", resource.kind)
    });
    row
}

/// Cached entries omit the original artifact because it is the source itself.
fn restore_original_artifact(
    hit: &mut ResourceAnalysis,
    out: &Path,
    index: usize,
    bytes: &[u8],
    resource: &Resource,
) -> Result<()> {
    if hit.status == "candidates_available" {
        let artifact = PathBuf::from(format!("originals/{index}.{}", resource.format));
        crate::filesystem::write_artifact(&out.join(&artifact), bytes)?;
        hit.original_artifact = Some(artifact);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_budget_serializes_oversized_work_without_deadlock() {
        let budget = PixelBudget::new(100);
        let peak = AtomicUsize::new(0);
        let current = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..6 {
                scope.spawn(|| {
                    // Requests above capacity are clamped instead of waiting forever.
                    let _lease = budget.acquire(1_000);
                    let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    current.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        let _a = budget.acquire(60);
        let _b = budget.acquire(40);
    }

    #[test]
    fn options_reject_out_of_range_policy_values() {
        for options in [
            AnalysisOptions {
                min_score: 101.0,
                ..Default::default()
            },
            AnalysisOptions {
                min_score: f64::NAN,
                ..Default::default()
            },
            AnalysisOptions {
                jobs: 17,
                ..Default::default()
            },
            AnalysisOptions {
                android_min_sdk: Some(0),
                ..Default::default()
            },
        ] {
            assert!(options.validate().is_err());
        }
        assert!(AnalysisOptions::default().validate().is_ok());
        assert!((1..=8).contains(&AnalysisOptions::default().worker_count()));
    }
}
