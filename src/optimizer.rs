use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

pub(crate) const MAX_INPUT: usize = 64 * 1024 * 1024;
const MAX_DECODED: usize = 256 * 1024 * 1024;

/// Lossless PNG policy. Unknown fields are rejected (including lossy settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub png_level: u8,
    pub include_ignored: bool,
    pub min_input_bytes: u64,
    pub min_savings_bytes: u64,
    pub min_savings_percent: f64,
    /// Allow lossless bit-depth, color-type and palette reductions. These keep
    /// every decoded RGBA sample but rewrite IHDR/PLTE/tRNS, so candidates are
    /// verified on expanded pixels instead of raw buffers.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reductions: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            png_level: 2,
            include_ignored: false,
            min_input_bytes: 50 * 1024,
            min_savings_bytes: 1024,
            min_savings_percent: 1.0,
            reductions: false,
        }
    }
}

impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.png_level <= 6,
            "png_level must be 0..=6 (effort, not quality)"
        );
        ensure!(
            self.min_savings_percent.is_finite()
                && (0.0..=100.0).contains(&self.min_savings_percent),
            "min_savings_percent must be 0..=100"
        );
        Ok(())
    }
}

pub(crate) fn optimize(original: &[u8], policy: &Policy) -> Result<Vec<u8>> {
    policy.validate()?;
    ensure!(original.len() <= MAX_INPUT, "input exceeds 64 MiB limit");
    let chunks = chunks(original)?;
    ensure!(
        !chunks.iter().any(|(kind, _)| kind == b"acTL"),
        "animated PNG is not supported yet"
    );
    let strict = encode(original, policy.png_level, false)?;
    verify(original, &strict, false)?;
    if !policy.reductions {
        return Ok(strict);
    }
    // A reduction that cannot be verified, or does not help, yields to strict.
    let reduced = encode(original, policy.png_level, true)
        .and_then(|candidate| verify(original, &candidate, true).map(|()| candidate));
    Ok(match reduced {
        Ok(reduced) if reduced.len() < strict.len() => reduced,
        _ => strict,
    })
}

fn encode(original: &[u8], level: u8, reductions: bool) -> Result<Vec<u8>> {
    let mut options = oxipng::Options::from_preset(level);
    options.optimize_alpha = false;
    options.bit_depth_reduction = reductions;
    options.color_type_reduction = reductions;
    options.palette_reduction = reductions;
    options.grayscale_reduction = reductions;
    options.scale_16 = false;
    options.interlace = None;
    options.strip = oxipng::StripChunks::None;
    options.max_decompressed_size = Some(MAX_DECODED);
    #[cfg(not(target_arch = "wasm32"))]
    {
        options.timeout = Some(Duration::from_secs(30));
    }
    // Browser hosts cancel the Worker. std::time::Instant has no clock on
    // wasm32-unknown-unknown; leave OxiPNG's native deadline disabled there.
    #[cfg(target_arch = "wasm32")]
    {
        options.timeout = None;
    }
    Ok(oxipng::optimize_from_memory(original, &options)?)
}

/// Chunks a lossless reduction may rewrite; everything else must be identical.
const REDUCIBLE_CHUNKS: [[u8; 4]; 3] = [*b"IHDR", *b"PLTE", *b"tRNS"];

/// Independent decoding plus exact chunk comparison. Strict mode rejects any
/// non-IDAT rewrite, even a harmless one. With `reductions`, IHDR/PLTE/tRNS may
/// change as long as dimensions, interlacing and expanded RGBA samples do not.
pub(crate) fn verify(original: &[u8], candidate: &[u8], reductions: bool) -> Result<()> {
    ensure!(
        original.len() <= MAX_INPUT && candidate.len() <= MAX_INPUT,
        "input exceeds 64 MiB limit"
    );
    let original_chunks = chunks(original)?;
    ensure!(
        !original_chunks.iter().any(|(kind, _)| kind == b"acTL"),
        "animated PNG is not supported yet"
    );
    let candidate_chunks = chunks(candidate)?;
    if !reductions {
        ensure!(
            original_chunks == candidate_chunks,
            "non-IDAT chunks changed; candidate rejected"
        );
        ensure!(
            decode(original)? == decode(candidate)?,
            "decoded pixels changed; candidate rejected"
        );
        return Ok(());
    }
    ensure!(
        fixed_chunks(&original_chunks) == fixed_chunks(&candidate_chunks),
        "chunks other than IHDR/PLTE/tRNS changed; candidate rejected"
    );
    ensure!(
        layout(&original_chunks)? == layout(&candidate_chunks)?,
        "dimensions or interlacing changed; candidate rejected"
    );
    crate::png_pixels::ensure_same_rgba(original, candidate, MAX_DECODED)
}

fn fixed_chunks<'a>(chunks: &[Chunk<'a>]) -> Vec<Chunk<'a>> {
    chunks
        .iter()
        .filter(|(kind, _)| !REDUCIBLE_CHUNKS.contains(kind))
        .copied()
        .collect()
}

/// Width, height and the compression/filter/interlace bytes of IHDR.
fn layout<'a>(chunks: &[Chunk<'a>]) -> Result<(&'a [u8], &'a [u8])> {
    let (kind, header) = chunks.first().context("PNG has no chunks")?;
    ensure!(kind == b"IHDR" && header.len() == 13, "invalid IHDR");
    Ok((&header[..8], &header[10..]))
}

type Chunk<'a> = ([u8; 4], &'a [u8]);

fn chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>> {
    ensure!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG");
    let mut offset = 8;
    let mut result = Vec::new();
    let mut saw_idat = false;
    while offset < bytes.len() {
        ensure!(bytes.len() - offset >= 12, "truncated PNG chunk");
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into()?) as usize;
        let end = offset
            .checked_add(length)
            .and_then(|end| end.checked_add(12))
            .context("PNG chunk overflow")?;
        ensure!(end <= bytes.len(), "truncated PNG chunk payload");
        let kind: [u8; 4] = bytes[offset + 4..offset + 8].try_into()?;
        if kind == *b"IDAT" && !saw_idat {
            result.push((kind, &bytes[0..0]));
            saw_idat = true;
        } else if kind != *b"IDAT" {
            result.push((kind, &bytes[offset + 8..end - 4]));
        }
        offset = end;
        if kind == *b"IEND" {
            ensure!(offset == bytes.len(), "data after IEND is not supported");
            return Ok(result);
        }
    }
    bail!("PNG has no IEND")
}

fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: MAX_DECODED });
    let mut reader = decoder.read_info()?;
    let size = reader
        .output_buffer_size()
        .context("PNG output buffer overflow")?;
    ensure!(size <= MAX_DECODED, "decoded image exceeds 256 MiB limit");
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer)?;
    buffer.truncate(frame.buffer_size());
    reader.finish()?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIDE: u32 = 64;

    /// Opaque two-color RGBA: reducible to a 1-bit palette without pixel loss.
    fn reducible_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, SIDE, SIDE);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .add_text_chunk("Comment".into(), "keep me".into())
                .unwrap();
            let data: Vec<u8> = (0..SIDE * SIDE)
                .flat_map(|i| {
                    if (i / 8 + i / SIDE / 8).is_multiple_of(2) {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    }
                })
                .collect();
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&data)
                .unwrap();
        }
        bytes
    }

    fn policy(reductions: bool) -> Policy {
        Policy {
            reductions,
            ..Policy::default()
        }
    }

    #[test]
    fn reductions_shrink_further_and_need_reduced_verification() {
        let original = reducible_png();
        let strict = optimize(&original, &policy(false)).unwrap();
        let reduced = optimize(&original, &policy(true)).unwrap();
        assert!(reduced.len() < strict.len());
        verify(&original, &reduced, true).unwrap();
        let error = verify(&original, &reduced, false).unwrap_err();
        assert!(error.to_string().contains("non-IDAT chunks changed"));
    }

    #[test]
    fn strict_candidates_pass_both_verifications() {
        let original = reducible_png();
        let strict = optimize(&original, &policy(false)).unwrap();
        verify(&original, &strict, false).unwrap();
        verify(&original, &strict, true).unwrap();
    }

    #[test]
    fn reductions_keep_ancillary_chunks() {
        let original = reducible_png();
        let reduced = optimize(&original, &policy(true)).unwrap();
        let text = |bytes| {
            chunks(bytes)
                .unwrap()
                .into_iter()
                .find(|(kind, _)| kind == b"tEXt")
                .map(|(_, data)| data.to_vec())
        };
        assert!(text(&original).is_some());
        assert_eq!(text(&original), text(&reduced));
    }

    #[test]
    fn reduced_verification_rejects_other_images() {
        let original = reducible_png();
        let mut other = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut other, SIDE, SIDE);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .add_text_chunk("Comment".into(), "keep me".into())
                .unwrap();
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![0; (SIDE * SIDE) as usize])
                .unwrap();
        }
        let error = verify(&original, &other, true).unwrap_err();
        assert!(error.to_string().contains("expanded pixels changed"));
    }

    #[test]
    fn reductions_never_produce_a_larger_file_than_strict_mode() {
        // Tiny image: the palette costs more than it saves.
        let mut original = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut original, 4, 4);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let data: Vec<u8> = (0..16u8).flat_map(|i| [i, 0, 0, 255]).collect();
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&data)
                .unwrap();
        }
        let strict = optimize(&original, &policy(false)).unwrap();
        let reduced = optimize(&original, &policy(true)).unwrap();
        assert!(reduced.len() <= strict.len());
    }

    #[test]
    fn default_policy_serializes_without_reductions() {
        let json = serde_json::to_string(&Policy::default()).unwrap();
        assert!(!json.contains("reductions"), "{json}");
        let enabled = serde_json::to_string(&policy(true)).unwrap();
        assert!(enabled.contains("\"reductions\":true"), "{enabled}");
    }
}
