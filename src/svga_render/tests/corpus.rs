//! Real files, when a directory of them is at hand:
//!
//! ```sh
//! RESOPT_SVGA_SAMPLES=dir [RESOPT_SVGA_RENDER_OUT=dir] \
//!     cargo test --release --lib svga_render::tests::corpus -- --ignored --nocapture
//! ```
use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SAMPLES: &str = "RESOPT_SVGA_SAMPLES";
const RENDER_OUT: &str = "RESOPT_SVGA_RENDER_OUT";
const MAX_SIDE: u32 = 256;
/// Share of files that must show at least one pixel.
const REQUIRED_VISIBLE: f64 = 0.9;

struct Outcome {
    frames: usize,
    visible: bool,
    /// Rasterizing alone, without reading and preparing the file.
    rendering: Duration,
}

fn outcome(path: &Path, out: Option<&Path>) -> anyhow::Result<Outcome> {
    let renderer = Renderer::new(&std::fs::read(path)?)?;
    let last = renderer.frame_count() - 1;
    let started = Instant::now();
    let poster = renderer.render(renderer.poster_frame(), MAX_SIDE)?;
    let ending = renderer.render(last, MAX_SIDE)?;
    let rendering = started.elapsed();
    if let (Some(out), Some(stem)) = (out, path.file_stem()) {
        let name = format!("{}-poster.png", stem.to_string_lossy());
        std::fs::write(out.join(name), crate::svga_render::encode_png(&poster)?)?;
    }
    Ok(Outcome {
        frames: 2,
        visible: !is_blank(&poster) || !is_blank(&ending),
        rendering,
    })
}

#[test]
#[ignore = "needs RESOPT_SVGA_SAMPLES"]
fn real_files_render_to_something_visible() {
    let Some(samples) = std::env::var_os(SAMPLES).map(PathBuf::from) else {
        return;
    };
    let out = std::env::var_os(RENDER_OUT).map(PathBuf::from);
    if let Some(out) = &out {
        std::fs::create_dir_all(out).unwrap();
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&samples)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "svga")
        })
        .collect();
    files.sort();

    let started = Instant::now();
    let outcomes: Vec<_> = files
        .iter()
        .map(|path| (path, outcome(path, out.as_deref())))
        .collect();
    let elapsed = started.elapsed();

    let failures: Vec<String> = outcomes
        .iter()
        .filter_map(|(path, outcome)| {
            let name = path.file_name()?.to_string_lossy();
            match outcome {
                Err(error) => Some(format!("{name}: {error}")),
                Ok(outcome) if !outcome.visible => Some(format!("{name}: blank")),
                Ok(_) => None,
            }
        })
        .collect();
    let rendered = || {
        outcomes
            .iter()
            .filter_map(|(_, outcome)| outcome.as_ref().ok())
    };
    let frames: usize = rendered().map(|outcome| outcome.frames).sum();
    let rendering: Duration = rendered().map(|outcome| outcome.rendering).sum();
    println!(
        "svga_render corpus: {} files, {} failures {:?}, {} frames at {:.2} ms each, {:.1?} in total with parsing",
        files.len(),
        failures.len(),
        failures,
        frames,
        rendering.as_secs_f64() * 1000.0 / frames.max(1) as f64,
        elapsed,
    );
    let visible = files.len() - failures.len();
    assert!(
        visible as f64 >= files.len() as f64 * REQUIRED_VISIBLE,
        "only {visible} of {} files rendered something",
        files.len()
    );
}
