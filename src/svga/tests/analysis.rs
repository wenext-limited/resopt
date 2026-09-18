//! `analyze` against a temporary project, plus the opt-in sample corpus.
use super::{super::*, sample};
use crate::analysis::{AnalysisControl, AnalysisOptions, PixelBudget};

struct Project {
    root: tempfile::TempDir,
    out: tempfile::TempDir,
    options: AnalysisOptions,
}

impl Project {
    fn new(options: AnalysisOptions) -> Self {
        let out = tempfile::tempdir().unwrap();
        for directory in ["candidates", "originals"] {
            std::fs::create_dir(out.path().join(directory)).unwrap();
        }
        Self {
            root: tempfile::tempdir().unwrap(),
            out,
            options,
        }
    }

    fn analyze(&self, bytes: &[u8], control: &AnalysisControl) -> ResourceAnalysis {
        std::fs::write(self.root.path().join("a.svga"), bytes).unwrap();
        let timings = crate::timings::Timings::default();
        let budget = PixelBudget::new(self.options.max_pixels);
        let context = Context {
            root: self.root.path(),
            out: self.out.path(),
            options: &self.options,
            min_sdk: None,
            timings: &timings,
            control,
            budget: &budget,
        };
        let resource = Resource::for_tests("a.svga", "svga");
        analyze(&context, &resource, 7, bytes, &hash(bytes))
    }
}

fn eager() -> AnalysisOptions {
    AnalysisOptions {
        min_input_bytes: 0,
        min_savings_bytes: 1,
        ..Default::default()
    }
}

#[test]
fn analyze_writes_one_verified_candidate() {
    let project = Project::new(eager());
    let original = sample();
    let row = project.analyze(&original, &AnalysisControl::default());
    assert_eq!(row.status, "candidates_available", "{:?}", row.issues);
    assert_eq!(row.sha256, Some(hash(&original)));
    assert_eq!(row.smallest_candidate, Some(0));
    let candidate = &row.candidates[0];
    assert_eq!(candidate.format, "svga");
    assert!(candidate.valid && !candidate.lossy && candidate.preview.is_none());
    assert_eq!(candidate.notes, ["embedded_pngs_optimized: 2/2"]);
    let artifact = candidate.artifact.as_ref().unwrap();
    assert_eq!(artifact, &PathBuf::from("candidates/7-svga-0.svga"));
    let written = std::fs::read(project.out.path().join(artifact)).unwrap();
    assert_eq!(candidate.sha256, Some(hash(&written)));
    assert_eq!(candidate.bytes, written.len() as u64);
    assert_eq!(
        candidate.savings_bytes,
        (original.len() - written.len()) as u64
    );
    verify(&original, &written).unwrap();
    let kept = row.original_artifact.unwrap();
    assert_eq!(kept, PathBuf::from("originals/7.svga"));
    assert_eq!(
        std::fs::read(project.out.path().join(kept)).unwrap(),
        original
    );
}

#[test]
fn analyze_reports_unsupported_failed_and_small_savings() {
    let project = Project::new(eager());
    let control = AnalysisControl::default();
    let zip = project.analyze(b"PK\x03\x04", &control);
    assert_eq!(zip.status, "unsupported");
    assert_eq!(zip.issues, ["svga_1x_zip_not_supported"]);
    let garbage = project.analyze(b"garbage", &control);
    assert_eq!(garbage.status, "failed");
    assert_eq!(garbage.issues, ["svga_not_zlib"]);
    assert!(garbage.sha256.is_some() && garbage.candidates.is_empty());

    let demanding = Project::new(AnalysisOptions {
        min_savings_bytes: u64::MAX,
        ..eager()
    });
    let row = demanding.analyze(&sample(), &control);
    assert_eq!(row.status, "inspected");
    assert!(row.candidates.is_empty() && row.original_artifact.is_none());
    assert_eq!(
        std::fs::read_dir(demanding.out.path().join("candidates"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn analyze_stops_when_cancelled() {
    let project = Project::new(eager());
    let control = AnalysisControl::default();
    control.cancel();
    let row = project.analyze(&sample(), &control);
    assert_eq!(row.status, "not_analyzed");
    assert!(row.candidates.is_empty() && row.issues.is_empty());
}

/// `RESOPT_SVGA_SAMPLES=<dir> cargo test --release svga_samples -- --ignored --nocapture`
#[test]
#[ignore = "needs RESOPT_SVGA_SAMPLES pointing at a directory of .svga files"]
fn svga_samples_round_trip() {
    let Some(directory) = std::env::var_os("RESOPT_SVGA_SAMPLES") else {
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "svga"))
        .collect();
    paths.sort();
    for reductions in [false, true] {
        let policy = Policy {
            reductions,
            ..Policy::default()
        };
        println!("png reductions: {reductions}");
        round_trip(&paths, &policy);
    }
}

fn round_trip(paths: &[PathBuf], policy: &Policy) {
    let (mut before, mut after, mut refused) = (0, 0, 0);
    for path in paths {
        let original = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        match optimize(&original, policy) {
            Ok(optimized) => {
                verify(&original, &optimized).unwrap();
                assert!(optimized.len() <= original.len());
                before += original.len();
                after += optimized.len();
                println!("{name}: {} -> {}", original.len(), optimized.len());
            }
            Err(error) => {
                assert!(
                    error.downcast_ref::<Refusal>().is_some(),
                    "{name}: {error:#}"
                );
                refused += 1;
                println!("{name}: refused: {error}");
            }
        }
    }
    let saved = 100.0 * (1.0 - after as f64 / before.max(1) as f64);
    println!(
        "{} files, {refused} refused, {before} -> {after} bytes ({saved:.2}% saved)",
        paths.len()
    );
}
