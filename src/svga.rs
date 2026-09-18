//! Verified lossless optimization of SVGA 2.x animations.
//!
//! The protobuf payload is rewritten at wire level: every field is copied
//! byte for byte except embedded PNG values, which go through the lossless PNG
//! optimizer, and the result is recompressed as one zlib stream. Anything
//! outside that known-safe subset (SVGA 1.x zip, audio, unknown fields,
//! animated or non-PNG images) is refused instead of rewritten.
mod container;
mod movie;
mod wire;

#[cfg(test)]
mod tests;

use crate::{
    ImageCandidate, Policy, Resource, ResourceAnalysis,
    analyze_image::Context,
    filesystem::{hash, write_new},
    optimizer,
    timings::Phase,
};
use anyhow::{Result, bail, ensure};
use movie::{Part, ValueKind};
use std::{fmt, path::PathBuf};

const MAX_INPUT: usize = optimizer::MAX_INPUT;
const MAX_INFLATED: usize = 256 * 1024 * 1024;
const CANCELLED: &str = "svga_analysis_cancelled";

/// Why a file is left alone. The reason is a stable machine-readable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A valid animation outside the subset that can be rewritten safely.
    Unsupported(&'static str),
    Malformed(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Refusal::Unsupported(reason) | Refusal::Malformed(reason)) = self;
        formatter.write_str(reason)
    }
}

impl std::error::Error for Refusal {}

fn inflate(bytes: &[u8]) -> Result<Vec<u8>, Refusal> {
    if bytes.len() > MAX_INPUT {
        return Err(Refusal::Unsupported("svga_input_exceeds_limit"));
    }
    container::inflate(bytes, MAX_INFLATED)
}

struct Optimized {
    bytes: Vec<u8>,
    images: usize,
    images_optimized: usize,
}

/// The smallest verified-safe encoding found, or the original bytes.
// Entry point for callers outside analysis; remove the allow once one exists.
#[allow(dead_code)]
pub(crate) fn optimize(original: &[u8], png: &Policy) -> Result<Vec<u8>> {
    Ok(optimize_with(original, png, &|| false)?.bytes)
}

fn optimize_with(original: &[u8], png: &Policy, cancelled: &dyn Fn() -> bool) -> Result<Optimized> {
    let proto = inflate(original)?;
    let parts = movie::parse(&proto)?;
    let mut rebuilt = Vec::with_capacity(proto.len());
    let (mut images, mut images_optimized) = (0, 0);
    for part in &parts {
        ensure!(!cancelled(), CANCELLED);
        let smaller = match part {
            Part::Image(_, image) if image.kind == ValueKind::Png => {
                images += 1;
                // A PNG the optimizer cannot handle stays as it is.
                optimizer::optimize(image.value, png)
                    .ok()
                    .filter(|candidate| candidate.len() < image.value.len())
                    .map(|candidate| movie::with_value(image, &candidate))
            }
            _ => None,
        };
        images_optimized += usize::from(smaller.is_some());
        rebuilt.extend_from_slice(smaller.as_deref().unwrap_or(part.field().raw));
    }
    let candidate = container::deflate(&rebuilt)?;
    let bytes = if candidate.len() < original.len() {
        candidate
    } else {
        original.to_vec()
    };
    Ok(Optimized {
        bytes,
        images,
        images_optimized,
    })
}

/// Independent check of the final bytes: same fields in the same order, every
/// non-image field and image key byte-identical, and every rewritten value a
/// PNG with identical pixels and ancillary chunks.
pub(crate) fn verify(original: &[u8], candidate: &[u8]) -> Result<()> {
    let (before_proto, after_proto) = (inflate(original)?, inflate(candidate)?);
    let (before, after) = (movie::parse(&before_proto)?, movie::parse(&after_proto)?);
    ensure!(
        before.len() == after.len(),
        "svga_verify_field_count_changed"
    );
    for (old, new) in before.iter().zip(&after) {
        match (old, new) {
            (Part::Verbatim(old), Part::Verbatim(new)) => {
                ensure!(old.raw == new.raw, "svga_verify_field_changed");
            }
            (Part::Image(_, old), Part::Image(_, new)) => {
                ensure!(
                    old.key == new.key && movie::entry_order(old) == movie::entry_order(new),
                    "svga_verify_image_key_changed"
                );
                if old.value == new.value {
                    continue;
                }
                ensure!(
                    old.kind == ValueKind::Png && new.kind == ValueKind::Png,
                    "svga_verify_image_value_changed"
                );
                // Strict first; a lossless reduction may rewrite IHDR/PLTE/tRNS.
                let same = optimizer::verify(old.value, new.value, false)
                    .or_else(|_| optimizer::verify(old.value, new.value, true));
                ensure!(same.is_ok(), "svga_verify_image_pixels_changed");
            }
            _ => bail!("svga_verify_field_order_changed"),
        }
    }
    Ok(())
}

pub(crate) fn analyze(
    context: &Context<'_>,
    resource: &Resource,
    index: usize,
    original: &[u8],
    digest: &str,
) -> ResourceAnalysis {
    let mut result = ResourceAnalysis::new(resource, "inspected");
    result.sha256 = Some(digest.to_string());
    if let Err(error) = analyze_into(context, resource, index, original, &mut result) {
        result.candidates.clear();
        result.smallest_candidate = None;
        result.status = match error.downcast_ref::<Refusal>() {
            Some(Refusal::Unsupported(_)) => "unsupported",
            _ if error.to_string() == CANCELLED => "not_analyzed",
            _ => "failed",
        }
        .into();
        if result.status != "not_analyzed" {
            result.issues.push(format!("{error:#}"));
        }
    }
    result
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
        control,
        ..
    } = context;
    // Refusals are reported even when no optimization is attempted.
    timings.time(Phase::Decode, || -> Result<()> {
        movie::parse(&inflate(original)?)?;
        Ok(())
    })?;
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
    let optimized = timings.time(Phase::EncodeLossless, || {
        // Players decode embedded PNGs to bitmaps, so palette and bit-depth
        // reductions are always allowed here; `verify` still requires identical
        // RGBA samples and untouched ancillary chunks.
        let policy = Policy {
            reductions: true,
            ..options.png_policy()
        };
        optimize_with(original, &policy, &|| control.is_cancelled())
    })?;
    let bytes = optimized.bytes;
    let savings_bytes = (original.len() as u64).saturating_sub(bytes.len() as u64);
    if savings_bytes < options.min_savings_bytes.max(1) {
        return Ok(());
    }
    timings.time(Phase::Decode, || verify(original, &bytes))?;
    let artifact = PathBuf::from(format!("candidates/{index}-svga-0.svga"));
    let original_artifact = PathBuf::from(format!("originals/{index}.svga"));
    timings.time(Phase::Write, || -> Result<()> {
        write_new(&out.join(&artifact), &bytes)?;
        write_new(&out.join(&original_artifact), original)
    })?;
    result.candidates.push(ImageCandidate {
        format: "svga".into(),
        quality: None,
        lossy: false,
        bytes: bytes.len() as u64,
        savings_bytes,
        valid: true,
        rejection: None,
        difference: None,
        artifact: Some(artifact),
        preview: None,
        sha256: Some(hash(&bytes)),
        warnings: vec![],
        notes: vec![format!(
            "embedded_pngs_optimized: {}/{}",
            optimized.images_optimized, optimized.images
        )],
    });
    result.smallest_candidate = Some(0);
    result.original_artifact = Some(original_artifact);
    result.status = "candidates_available".into();
    // Report a moving source instead of presenting stale candidate estimates.
    let current = crate::filesystem::contained_file(context.root, &resource.path)
        .and_then(|path| crate::resources::bounded_read(&path))?;
    ensure!(
        Some(hash(&current)) == result.sha256,
        "source_changed_during_analysis"
    );
    Ok(())
}
