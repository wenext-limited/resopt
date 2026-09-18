//! Native libwebp integration; the browser WASM target keeps its PNG-only engine.
use crate::image_backend::Decoded;
use anyhow::Context;
use anyhow::{Result, ensure};

pub(crate) fn encode(original: &[u8], image: &Decoded, quality: u8) -> Result<Vec<u8>> {
    ensure!((1..=100).contains(&quality), "invalid_webp_quality");
    ensure!(
        image.info.frames == 1 && image.info.orientation == 1,
        "webp_requires_static_upright_image"
    );
    ensure!(
        image.info.width <= 16383 && image.info.height <= 16383,
        "webp_dimensions_exceed_codec_limit"
    );
    if !cfg!(target_os = "macos") && original.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut offset = 8usize;
        while offset + 12 <= original.len() {
            let len = u32::from_be_bytes(original[offset..offset + 4].try_into()?) as usize;
            let tag = &original[offset + 4..offset + 8];
            ensure!(
                !matches!(tag, b"iCCP" | b"cHRM" | b"gAMA" | b"eXIf"),
                "color_profile_or_orientation_requires_macos"
            );
            offset = offset
                .checked_add(len)
                .and_then(|n| n.checked_add(12))
                .context("invalid_png_chunk")?;
        }
    }
    let mut rgba = Vec::with_capacity(image.pixels.len());
    for pixel in image.pixels.as_chunks::<4>().0 {
        let a = pixel[3].clamp(0.0, 1.0);
        for c in &pixel[..3] {
            rgba.push(
                (if a > 0.0 { c / a } else { 0.0 })
                    .clamp(0.0, 1.0)
                    .mul_add(255.0, 0.5) as u8,
            );
        }
        rgba.push((a * 255.0).round() as u8);
    }
    webp::Encoder::from_rgba(&rgba, image.info.width as u32, image.info.height as u32)
        .encode_simple(false, f32::from(quality))
        .map(|bytes| bytes.to_vec())
        .map_err(|e| anyhow::anyhow!("webp_encoding_failed: {e:?}"))
}

#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn decode(bytes: &[u8], max_pixels: usize) -> Result<Decoded> {
    use crate::ImageInfo;
    let features = webp::BitstreamFeatures::new(bytes).context("invalid_webp")?;
    ensure!(!features.has_animation(), "multiple_frames_not_transcoded");
    let (width, height) = (features.width() as usize, features.height() as usize);
    ensure!(
        width
            .checked_mul(height)
            .is_some_and(|n| n > 0 && n <= max_pixels.min(crate::MAX_PIXELS_LIMIT)),
        "decoded_image_exceeds_max_pixels"
    );
    if bytes.get(12..16) == Some(b"VP8X") {
        ensure!(
            bytes
                .get(20)
                .is_some_and(|flags| flags & (0x20 | 0x08) == 0),
            "webp_color_profile_or_orientation_requires_macos"
        );
    }
    let image = webp::Decoder::new(bytes)
        .decode()
        .context("webp_decode_failed")?;
    let channels = if image.is_alpha() { 4 } else { 3 };
    let mut transparent_pixels = 0;
    let mut pixels = Vec::with_capacity(width * height * 4);
    for p in image.chunks_exact(channels) {
        let a = if channels == 4 {
            f32::from(p[3]) / 255.0
        } else {
            1.0
        };
        transparent_pixels += usize::from(a < 1.0);
        pixels.extend([
            f32::from(p[0]) / 255.0 * a,
            f32::from(p[1]) / 255.0 * a,
            f32::from(p[2]) / 255.0 * a,
            a,
        ]);
    }
    Ok(Decoded {
        info: ImageInfo {
            decoder_type: "webp".into(),
            width,
            height,
            frames: 1,
            bits_per_component: 8,
            orientation: 1,
            transparent_pixels,
            has_transparent_pixels: transparent_pixels > 0,
        },
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_webp_decoder_preserves_alpha_and_bounds() {
        let rgba = [80, 120, 160, 128].repeat(64 * 64);
        let encoded = webp::Encoder::from_rgba(&rgba, 64, 64)
            .encode_simple(false, 85.0)
            .unwrap();
        let image = decode(&encoded, 4096).unwrap();
        assert_eq!(image.info.transparent_pixels, 4096);
        assert!((image.pixels[3] - 128.0 / 255.0).abs() < 1e-6);
        assert!(decode(&encoded, 4095).is_err());
        assert!(decode(b"not webp", 4096).is_err());
    }
}
