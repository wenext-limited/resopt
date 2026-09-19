//! Verified lossless optimization of SVGA 2.x animations.
//!
//! The `svga` crate keeps every protobuf field as its stored bytes, so the
//! payload is copied byte for byte except embedded PNG values, which go through
//! the lossless PNG optimizer, and recompressed as one zlib stream. Anything
//! outside that known-safe subset (SVGA 1.x zip, audio, unknown fields,
//! animated or non-PNG images) is refused instead of rewritten.
mod rules;

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
use rules::IMAGES;
pub(crate) use rules::Refusal;
use std::path::PathBuf;
use svga::{Compression, ValueKind};

const CANCELLED: &str = "svga_analysis_cancelled";

struct Optimized {
    bytes: Vec<u8>,
    images: usize,
    images_optimized: usize,
}

/// The smallest verified-safe encoding found, or the original bytes.
/// One-shot optimization without cancellation; analysis uses `optimize_with`.
#[cfg(test)]
pub(crate) fn optimize(original: &[u8], png: &Policy) -> Result<Vec<u8>> {
    Ok(optimize_with(original, png, &|| false)?.bytes)
}

fn optimize_with(original: &[u8], png: &Policy, cancelled: &dyn Fn() -> bool) -> Result<Optimized> {
    let document = rules::open(original)?;
    let mut edited = document.clone();
    let (mut images, mut images_optimized) = (0, 0);
    // Entries are addressed by position, which also reaches keys that are not
    // valid UTF-8. Replacing a value never moves an entry, so positions in
    // `document` stay valid for `edited`.
    for (position, image) in document.images().enumerate() {
        ensure!(!cancelled(), CANCELLED);
        if image.kind() != ValueKind::Png {
            continue;
        }
        images += 1;
        // A PNG the optimizer cannot handle stays as it is.
        let smaller = optimizer::optimize(image.value(), png)
            .ok()
            .filter(|candidate| candidate.len() < image.value().len());
        if let Some(candidate) = smaller {
            edited = edited
                .replace_image_at(position, &candidate)
                .map_err(Refusal::from)?;
            images_optimized += 1;
        }
    }
    ensure!(!cancelled(), CANCELLED);
    let candidate = edited.to_bytes(Compression::Best).map_err(Refusal::from)?;
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
    let (before, after) = (rules::open(original)?, rules::open(candidate)?);
    ensure!(
        before.fields().count() == after.fields().count(),
        "svga_verify_field_count_changed"
    );
    let (mut old_images, mut new_images) = (before.images(), after.images());
    for (old, new) in before.fields().zip(after.fields()) {
        match (old.number == IMAGES, new.number == IMAGES) {
            (false, false) => ensure!(old.raw == new.raw, "svga_verify_field_changed"),
            (true, true) => {
                let (Some(old), Some(new)) = (old_images.next(), new_images.next()) else {
                    bail!("svga_verify_field_order_changed");
                };
                ensure!(
                    old.key_bytes() == new.key_bytes()
                        && old.field_numbers() == new.field_numbers(),
                    "svga_verify_image_key_changed"
                );
                if old.value() == new.value() {
                    continue;
                }
                ensure!(
                    old.kind() == ValueKind::Png && new.kind() == ValueKind::Png,
                    "svga_verify_image_value_changed"
                );
                // Strict first; a lossless reduction may rewrite IHDR/PLTE/tRNS.
                let same = optimizer::verify(old.value(), new.value(), false)
                    .or_else(|_| optimizer::verify(old.value(), new.value(), true));
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
    timings.time(Phase::Decode, || rules::open(original).map(drop))?;
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
