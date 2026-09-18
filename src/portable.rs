//! In-memory operations shared by the native CLI and browser adapters.
//! No filesystem, processes, sockets, or platform image framework is used here.
use crate::{
    image_backend::{Decoded, ImageInfo},
    optimizer, png_pixels, quality,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;

pub const WEB_MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const WEB_MAX_PIXELS: usize = 2 * 1024 * 1024;
const DECODE_LIMIT: usize = WEB_MAX_PIXELS * 16;

#[derive(Debug, Serialize)]
pub struct PngSummary {
    pub width: usize,
    pub height: usize,
    pub original_bytes: usize,
    pub optimized_bytes: usize,
    pub saved_bytes: usize,
    pub transparent_pixels: usize,
    pub pixel_equivalent: bool,
    pub ssimulacra2: f64,
    pub reductions: bool,
}

pub struct OptimizedPng {
    pub bytes: Vec<u8>,
    pub summary: PngSummary,
}

/// Optimize a static PNG within the browser's conservative per-image budget.
/// Returns the original bytes when no smaller encoding is found.
pub fn optimize_png(bytes: &[u8], effort: u8, reductions: bool) -> Result<OptimizedPng> {
    ensure!(
        bytes.len() <= WEB_MAX_INPUT_BYTES,
        "PNG exceeds the 16 MiB browser input limit"
    );
    ensure!(effort <= 4, "browser effort must be 0..=4");
    let original = decode_png(bytes)?;
    let candidate = optimizer::optimize(
        bytes,
        &optimizer::Policy {
            png_level: effort,
            reductions,
            ..Default::default()
        },
    )?;
    optimizer::verify(bytes, &candidate, reductions)?;
    let chosen = if candidate.len() < bytes.len() {
        candidate
    } else {
        bytes.to_vec()
    };
    let decoded = decode_png(&chosen)?;
    let score = quality::ssimulacra2(&original, &decoded)?;
    Ok(OptimizedPng {
        summary: PngSummary {
            width: original.info.width,
            height: original.info.height,
            original_bytes: bytes.len(),
            optimized_bytes: chosen.len(),
            saved_bytes: bytes.len().saturating_sub(chosen.len()),
            transparent_pixels: original.info.transparent_pixels,
            pixel_equivalent: true,
            ssimulacra2: score,
            reductions,
        },
        bytes: chosen,
    })
}

fn decode_png(bytes: &[u8]) -> Result<Decoded> {
    let mut reader = png_pixels::reader(bytes, DECODE_LIMIT)?;
    let width = reader.info().width as usize;
    let height = reader.info().height as usize;
    let count = width
        .checked_mul(height)
        .context("PNG dimensions overflow")?;
    ensure!(
        count > 0 && count <= WEB_MAX_PIXELS,
        "PNG exceeds the 2 MP browser pixel limit; use the native CLI for larger images"
    );
    ensure!(
        reader.info().animation_control.is_none(),
        "animated PNG is not supported"
    );
    let mut pixels = Vec::with_capacity(count * 4);
    let mut transparent_pixels = 0;
    while let Some(row) = png_pixels::next_rgba_row(&mut reader)? {
        for pixel in row.as_chunks::<4>().0 {
            let alpha = f32::from(pixel[3]) / 65535.0;
            transparent_pixels += usize::from(pixel[3] != u16::MAX);
            pixels.extend([
                f32::from(pixel[0]) / 65535.0 * alpha,
                f32::from(pixel[1]) / 65535.0 * alpha,
                f32::from(pixel[2]) / 65535.0 * alpha,
                alpha,
            ]);
        }
    }
    ensure!(pixels.len() == count * 4, "incomplete PNG pixels");
    Ok(Decoded {
        info: ImageInfo {
            decoder_type: "png".into(),
            width,
            height,
            frames: 1,
            bits_per_component: 16,
            orientation: 1,
            transparent_pixels,
            has_transparent_pixels: transparent_pixels > 0,
        },
        pixels,
    })
}

/// Score caller-supplied straight-alpha, 8-bit sRGB RGBA buffers (ImageData).
/// Callers must normalize image color profiles to sRGB before using this API.
pub fn score_srgb_rgba(
    width: usize,
    height: usize,
    original: &[u8],
    candidate: &[u8],
) -> Result<f64> {
    let count = width
        .checked_mul(height)
        .context("image dimensions overflow")?;
    ensure!(
        count > 0 && count <= WEB_MAX_PIXELS,
        "comparison exceeds browser pixel limit"
    );
    ensure!(
        original.len() == count * 4 && candidate.len() == count * 4,
        "RGBA buffer length mismatch"
    );
    let decode = |bytes: &[u8]| {
        let transparent_pixels = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 255)
            .count();
        Decoded {
            info: ImageInfo {
                decoder_type: "sRGB ImageData".into(),
                width,
                height,
                frames: 1,
                bits_per_component: 8,
                orientation: 1,
                transparent_pixels,
                has_transparent_pixels: transparent_pixels > 0,
            },
            pixels: bytes
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = f32::from(p[3]) / 255.0;
                    [
                        f32::from(p[0]) / 255.0 * a,
                        f32::from(p[1]) / 255.0 * a,
                        f32::from(p[2]) / 255.0 * a,
                        a,
                    ]
                })
                .collect(),
        }
    };
    quality::ssimulacra2(&decode(original), &decode(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_compression(png::Compression::NoCompression);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[40, 80, 120, 128].repeat(64 * 64))
                .unwrap();
        }
        bytes
    }
    #[test]
    fn portable_optimization_preserves_alpha_and_never_grows_output() {
        let original = png();
        let first = optimize_png(&original, 2, true).unwrap();
        assert!(first.bytes.len() < original.len());
        assert_eq!(first.summary.transparent_pixels, 64 * 64);
        assert!((first.summary.ssimulacra2 - 100.0).abs() < 0.01);
        let second = optimize_png(&first.bytes, 2, true).unwrap();
        assert!(second.bytes.len() <= first.bytes.len());
    }
    #[test]
    fn malformed_inputs_and_rgba_lengths_are_rejected() {
        assert!(optimize_png(b"invalid", 2, true).is_err());
        assert!(optimize_png(&png(), 5, true).is_err());
        assert!(score_srgb_rgba(64, 64, &[0; 4], &[0; 4]).is_err());
        assert!(score_srgb_rgba(WEB_MAX_PIXELS + 1, 1, &[], &[]).is_err());
    }
}
