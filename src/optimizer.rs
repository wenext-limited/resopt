use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, time::Duration};

pub(crate) const MAX_INPUT: usize = 64 * 1024 * 1024;
const MAX_DECODED: usize = 256 * 1024 * 1024;

/// Lossless PNG policy. Unknown fields are rejected (including lossy settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub png_level: u8,
    pub min_input_bytes: u64,
    pub min_savings_bytes: u64,
    pub min_savings_percent: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            png_level: 2,
            min_input_bytes: 50 * 1024,
            min_savings_bytes: 1024,
            min_savings_percent: 1.0,
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
    let mut options = oxipng::Options::from_preset(policy.png_level);
    options.optimize_alpha = false;
    options.bit_depth_reduction = false;
    options.color_type_reduction = false;
    options.palette_reduction = false;
    options.grayscale_reduction = false;
    options.scale_16 = false;
    options.interlace = None;
    options.strip = oxipng::StripChunks::None;
    options.max_decompressed_size = Some(MAX_DECODED);
    options.timeout = Some(Duration::from_secs(30));
    let candidate = oxipng::optimize_from_memory(original, &options)?;
    verify(original, &candidate)?;
    Ok(candidate)
}

/// Independent decoding plus exact non-IDAT chunk comparison. This deliberately
/// rejects candidates that rewrite metadata, even if the rewrite seems harmless.
pub(crate) fn verify(original: &[u8], candidate: &[u8]) -> Result<()> {
    ensure!(
        original.len() <= MAX_INPUT && candidate.len() <= MAX_INPUT,
        "input exceeds 64 MiB limit"
    );
    let original_chunks = chunks(original)?;
    ensure!(
        !original_chunks.iter().any(|(kind, _)| kind == b"acTL"),
        "animated PNG is not supported yet"
    );
    ensure!(
        original_chunks == chunks(candidate)?,
        "non-IDAT chunks changed; candidate rejected"
    );
    ensure!(
        decode(original)? == decode(candidate)?,
        "decoded pixels changed; candidate rejected"
    );
    Ok(())
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
