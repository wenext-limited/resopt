//! Candidate generation and verification for one image resource.
use crate::{
    ImageCandidate, Resource, ResourceAnalysis,
    analysis::{AnalysisControl, AnalysisOptions, PixelBudget},
    android,
    filesystem::{hash, write_artifact},
    image_backend, optimizer,
    timings::{Phase, Timings},
    webp_backend,
};
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

/// Longest side of thumbnails written into the report.
const PREVIEW_SIDE: u32 = 256;

pub(crate) struct Context<'a> {
    pub root: &'a Path,
    pub out: &'a Path,
    pub options: &'a AnalysisOptions,
    pub min_sdk: Option<u32>,
    pub timings: &'a Timings,
    pub control: &'a AnalysisControl,
    pub budget: &'a PixelBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Trial {
    pub format: &'static str,
    /// `None` is a lossless encoding.
    pub quality: Option<u8>,
}

pub(crate) fn in_catalog(path: &Path) -> bool {
    path.components().any(|part| {
        Path::new(part.as_os_str())
            .extension()
            .is_some_and(|e| e == "xcassets")
    })
}

/// Everything about a resource, other than its bytes, that changes which
/// candidates are produced. Results are shared only between equal keys.
pub(crate) fn policy_key(resource: &Resource, min_sdk: Option<u32>) -> String {
    let class = match &resource.android {
        Some(android) => android.policy_key(min_sdk),
        None if resource.origin == "catalog_rendition" || in_catalog(&resource.path) => {
            "catalog".into()
        }
        None => "loose".into(),
    };
    format!(
        "{}|{}|{}|{}",
        resource.format,
        class,
        resource.format_lock.as_deref().unwrap_or("-"),
        resource.conversion_exclusion.as_deref().unwrap_or("-")
    )
}

/// The encodings worth trying, plus reasons some were ruled out.
pub(crate) fn trials(
    resource: &Resource,
    has_alpha: bool,
    options: &AnalysisOptions,
    min_sdk: Option<u32>,
) -> (Vec<Trial>, Vec<String>) {
    let mut trials = Vec::new();
    let mut issues = Vec::new();
    if resource.format == "png" {
        trials.push(Trial {
            format: "png",
            quality: None,
        });
    }
    if let Some(lock) = &resource.format_lock {
        issues.push(lock.clone());
        return (trials, issues);
    }
    let lossy = |format: &'static str| {
        options.qualities.iter().map(move |quality| Trial {
            format,
            quality: Some(*quality),
        })
    };
    let android = resource.android.is_some();
    if image_backend::image_backend_available() {
        if android {
            // Same-format recompression only; never new JPEG/HEIC files on Android.
            if resource.format == "jpeg" {
                trials.extend(lossy("jpeg"));
            }
        } else {
            if !has_alpha {
                trials.extend(lossy("jpeg"));
            }
            trials.extend(lossy("heic"));
        }
    }
    let webp_target = !in_catalog(&resource.path) && resource.origin != "catalog_rendition";
    if !options.webp && webp_target && (android || resource.format == "webp") {
        issues.push("webp_candidates_disabled".into());
    }
    if options.webp && webp_target {
        let gate = |lossless: bool| {
            if android {
                android::webp_compatibility(min_sdk, lossless, has_alpha)
            } else {
                Ok(())
            }
        };
        match gate(false) {
            Ok(()) => trials.extend(lossy("webp")),
            Err(reason) => issues.push(reason),
        }
        if resource.format == "png" {
            match gate(true) {
                Ok(()) => trials.push(Trial {
                    format: "webp",
                    quality: None,
                }),
                Err(reason) if !issues.contains(&reason) => issues.push(reason),
                Err(_) => {}
            }
        }
    }
    (trials, issues)
}

/// Pixel count from the file header, used to reserve memory before decoding.
fn header_pixels(bytes: &[u8]) -> Option<usize> {
    let be = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize);
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return be(16)?.checked_mul(be(20)?);
    }
    webp::BitstreamFeatures::new(bytes)
        .map(|features| features.width() as usize * features.height() as usize)
}

pub(crate) fn analyze(
    context: &Context<'_>,
    resource: &Resource,
    index: usize,
    original: &[u8],
    digest: &str,
) -> ResourceAnalysis {
    if resource.format == "svga" {
        let mut row = crate::svga::analyze(context, resource, index, original, digest);
        attach_animation_preview(context, &mut row, index, original);
        return row;
    }
    let mut result = ResourceAnalysis::new(resource, "inspected");
    result.sha256 = Some(digest.to_string());
    let attempt = analyze_into(context, resource, index, original, &mut result);
    if let Err(error) = attempt {
        result.status = "failed".into();
        result.issues.push(format!("{error:#}"));
        result.smallest_candidate = None;
    }
    result
}

/// Poster thumbnail and timing facts for an animation. Rendering problems
/// never fail the row: optimization is verified on bytes, not on this picture.
fn attach_animation_preview(
    context: &Context<'_>,
    row: &mut ResourceAnalysis,
    index: usize,
    original: &[u8],
) {
    if context.control.is_cancelled() || row.status == "not_analyzed" {
        return;
    }
    let rendered = context.timings.time(Phase::Preview, || -> Result<_> {
        let renderer = crate::svga_render::Renderer::new(original)?;
        let poster = renderer.poster_frame();
        let frame = renderer.render(poster, PREVIEW_SIDE)?;
        let (width, height) = renderer.output_size(u32::MAX);
        let info = crate::analysis::AnimationInfo {
            width,
            height,
            fps: renderer.fps(),
            frames: renderer.frame_count(),
            poster_frame: poster,
        };
        Ok((crate::svga_render::encode_png(&frame)?, info))
    });
    match rendered {
        Ok((png, info)) => {
            let preview = PathBuf::from(format!("previews/{index}-original.png"));
            if write_artifact(&context.out.join(&preview), &png).is_ok() {
                row.original_preview = Some(preview);
            }
            row.animation = Some(info);
        }
        Err(error) => row.issues.push(format!("preview_unavailable: {error:#}")),
    }
}

fn analyze_into(
    context: &Context<'_>,
    resource: &Resource,
    index: usize,
    original: &[u8],
    result: &mut ResourceAnalysis,
) -> Result<()> {
    let Context {
        out,
        options,
        timings,
        ..
    } = context;
    // Unknown headers reserve a quarter of the cap; the decoder enforces the cap.
    let _lease = context
        .budget
        .acquire(header_pixels(original).unwrap_or(options.max_pixels / 4));
    let decoded = timings.time(Phase::Decode, || {
        image_backend::decode(original, options.max_pixels)
    })?;
    result.image = Some(decoded.info.clone());
    result.fingerprint = crate::similarity::fingerprint(&decoded);
    // Every decoded image gets a thumbnail, not only those with candidates:
    // already-optimal, excluded and format-locked images must be viewable too.
    let preview = PathBuf::from(format!("previews/{index}-original.png"));
    let thumbnail = timings.time(Phase::Preview, || image_backend::preview(&decoded))?;
    timings.time(Phase::Write, || {
        write_artifact(&out.join(&preview), &thumbnail)
    })?;
    result.original_preview = Some(preview);
    if decoded.info.frames != 1 {
        result.issues.push("multiple_frames_not_transcoded".into());
        return Ok(());
    }
    if let Some(reason) = &resource.conversion_exclusion {
        result.status = "excluded".into();
        result.issues.push(reason.clone());
        return Ok(());
    }
    if options.probe_only {
        return Ok(());
    }
    if original.len() < options.min_input_bytes as usize {
        result.issues.push("below_explicit_input_threshold".into());
        return Ok(());
    }
    let (trials, issues) = trials(
        resource,
        decoded.info.has_transparent_pixels,
        options,
        context.min_sdk,
    );
    result.issues.extend(issues);
    let mut scorer = crate::quality::ReferenceScorer::new(&decoded);
    for trial in trials {
        if context.control.is_cancelled() {
            result.status = "not_analyzed".into();
            result.candidates.clear();
            return Ok(());
        }
        let Trial { format, quality } = trial;
        let mut candidate = ImageCandidate {
            format: format.into(),
            quality,
            lossy: quality.is_some(),
            bytes: 0,
            savings_bytes: 0,
            valid: false,
            rejection: None,
            difference: None,
            artifact: None,
            preview: None,
            sha256: None,
            warnings: vec![],
            notes: vec![],
        };
        let checked = (|| -> Result<()> {
            let phase = if quality.is_some() {
                Phase::EncodeLossy
            } else {
                Phase::EncodeLossless
            };
            let bytes = timings.time(phase, || match (format, quality) {
                ("webp", Some(quality)) => webp_backend::encode(original, &decoded, quality),
                ("webp", None) => webp_backend::encode_lossless(original, options.max_pixels),
                (_, Some(quality)) => image_backend::encode(original, format, quality),
                (_, None) => optimizer::optimize(original, &options.png_policy()),
            })?;
            candidate.bytes = bytes.len() as u64;
            candidate.savings_bytes = (original.len() as u64).saturating_sub(candidate.bytes);
            // A candidate that is not smaller is never scored, previewed or written.
            if candidate.savings_bytes < options.min_savings_bytes.max(1) {
                return Ok(());
            }
            let after = timings.time(Phase::Decode, || {
                image_backend::decode(&bytes, options.max_pixels)
            })?;
            let difference = timings.time(Phase::Score, || {
                image_backend::compare_with(&decoded, &after, || scorer.score(&after))
            })?;
            let checks = [
                (
                    "alpha_error_exceeds_policy",
                    difference.max_alpha_error > options.max_alpha_error,
                ),
                (
                    "transparency_presence_changed",
                    decoded.info.has_transparent_pixels != after.info.has_transparent_pixels,
                ),
                (
                    "quality_below_policy",
                    candidate.lossy
                        && difference
                            .ssimulacra2
                            .is_some_and(|score| score < options.min_score),
                ),
            ];
            candidate.warnings = checks
                .iter()
                .filter(|(_, missed)| *missed)
                .map(|(kind, _)| (*kind).to_string())
                .collect();
            candidate.rejection = candidate.warnings.first().cloned();
            ensure!(
                candidate.lossy || candidate.rejection.is_none(),
                "lossless_candidate_changed_pixels"
            );
            candidate.difference = Some(difference);
            candidate.valid = candidate.rejection.is_none();
            if format == "webp" {
                let dropped = webp_backend::dropped_png_metadata(original);
                if !dropped.is_empty() {
                    candidate
                        .notes
                        .push(format!("metadata_not_carried_over: {}", dropped.join(", ")));
                }
            }
            let stem = format!("{index}-{format}-{}", quality.unwrap_or(0));
            let artifact = PathBuf::from(format!("candidates/{stem}.{format}"));
            let preview = PathBuf::from(format!("previews/{stem}.png"));
            let thumbnail = timings.time(Phase::Preview, || image_backend::preview(&after))?;
            timings.time(Phase::Write, || -> Result<()> {
                write_artifact(&out.join(&artifact), &bytes)?;
                write_artifact(&out.join(&preview), &thumbnail)
            })?;
            candidate.sha256 = Some(hash(&bytes));
            candidate.artifact = Some(artifact);
            candidate.preview = Some(preview);
            Ok(())
        })();
        if let Err(error) = checked {
            candidate.valid = false;
            candidate.artifact = None;
            candidate.warnings.clear();
            candidate.rejection = Some(format!("{error:#}"));
        }
        result.candidates.push(candidate);
    }
    result.smallest_candidate = result
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.valid && candidate.artifact.is_some())
        .min_by_key(|(_, candidate)| candidate.bytes)
        .map(|(index, _)| index);
    if result.candidates.iter().any(|c| c.artifact.is_some()) {
        let artifact = PathBuf::from(format!("originals/{index}.{}", resource.format));
        timings.time(Phase::Write, || {
            write_artifact(&out.join(&artifact), original)
        })?;
        result.original_artifact = Some(artifact);
        result.status = "candidates_available".into();
    }
    // Report a moving source instead of presenting stale candidate estimates.
    let current = crate::filesystem::contained_file(context.root, &resource.path)
        .and_then(|path| crate::resources::bounded_read(&path))?;
    ensure!(
        Some(hash(&current)) == result.sha256,
        "source_changed_during_analysis"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(path: &str, format: &str) -> Resource {
        let mut resource = Resource::for_tests(path, format);
        resource.android = android::classify(Path::new(path));
        resource.format_lock = resource
            .android
            .as_ref()
            .and_then(|a| a.format_lock())
            .map(str::to_string);
        resource
    }

    fn formats(trials: &[Trial]) -> Vec<(&'static str, Option<u8>)> {
        trials.iter().map(|t| (t.format, t.quality)).collect()
    }

    #[test]
    fn android_resources_never_receive_heic_or_new_jpeg_files() {
        let options = AnalysisOptions {
            webp: true,
            qualities: vec![85],
            ..Default::default()
        };
        let drawable = resource("app/src/main/res/drawable-xxhdpi/bg.png", "png");
        let (found, issues) = trials(&drawable, true, &options, Some(21));
        assert_eq!(
            formats(&found),
            [("png", None), ("webp", Some(85)), ("webp", None)]
        );
        assert!(issues.is_empty());
        let (found, _) = trials(&drawable, false, &options, Some(21));
        assert!(found.iter().all(|t| !matches!(t.format, "heic" | "jpeg")));
    }

    #[test]
    fn webp_is_blocked_with_a_reason_below_the_required_api_level() {
        let options = AnalysisOptions {
            webp: true,
            qualities: vec![85],
            ..Default::default()
        };
        let drawable = resource("app/src/main/res/drawable/bg.png", "png");
        let (found, issues) = trials(&drawable, true, &options, Some(16));
        assert_eq!(formats(&found), [("png", None)]);
        assert_eq!(issues, ["android_min_sdk_16_below_webp_requirement_18"]);
        // Opaque lossy WebP is fine on API 16; lossless still needs 18.
        let (found, issues) = trials(&drawable, false, &options, Some(16));
        assert_eq!(formats(&found), [("png", None), ("webp", Some(85))]);
        assert_eq!(issues, ["android_min_sdk_16_below_webp_requirement_18"]);
        let (found, issues) = trials(&drawable, false, &options, None);
        assert_eq!(formats(&found), [("png", None)]);
        assert_eq!(issues, ["android_min_sdk_unknown"]);
    }

    #[test]
    fn format_locked_files_only_get_same_format_lossless_work() {
        let options = AnalysisOptions {
            webp: true,
            ..Default::default()
        };
        for path in [
            "app/src/main/res/drawable-xhdpi/bubble.9.png",
            "app/src/main/res/mipmap-xxhdpi/ic_launcher.png",
            "app/src/main/res/raw/splash.png",
        ] {
            let (found, issues) = trials(&resource(path, "png"), true, &options, Some(24));
            assert_eq!(formats(&found), [("png", None)], "{path}");
            assert_eq!(issues.len(), 1);
        }
        let (found, _) = trials(
            &resource("app/src/main/res/mipmap-xxhdpi/ic_launcher.webp", "webp"),
            true,
            &options,
            Some(24),
        );
        assert!(found.is_empty());
    }

    #[test]
    fn catalog_renditions_never_receive_webp_and_policy_keys_separate_classes() {
        let options = AnalysisOptions {
            webp: true,
            ..Default::default()
        };
        let catalog = resource("App/Assets.xcassets/a.imageset/a.png", "png");
        let (found, _) = trials(&catalog, false, &options, None);
        assert!(found.iter().all(|t| t.format != "webp"));
        let loose = resource("App/Resources/a.png", "png");
        let android = resource("app/src/main/res/drawable/a.png", "png");
        let keys = [
            policy_key(&catalog, None),
            policy_key(&loose, None),
            policy_key(&android, Some(21)),
            policy_key(&android, Some(16)),
            policy_key(
                &resource("app/src/main/res/drawable/a.9.png", "png"),
                Some(21),
            ),
        ];
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn header_pixels_reads_png_and_webp_dimensions() {
        let mut png_bytes = Vec::new();
        png::Encoder::new(&mut png_bytes, 7, 5)
            .write_header()
            .unwrap()
            .write_image_data(&[0; 7 * 5])
            .unwrap_or(());
        assert_eq!(header_pixels(&png_bytes), Some(35));
        let webp_bytes = webp::Encoder::from_rgba(&[1; 6 * 4 * 4], 6, 4)
            .encode_simple(false, 80.0)
            .unwrap();
        assert_eq!(header_pixels(&webp_bytes), Some(24));
        assert_eq!(header_pixels(b"not an image"), None);
    }
}
