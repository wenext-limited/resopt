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

/// PNG chunks that change how samples are interpreted. WebP candidates written
/// by this backend carry no profile, so such sources cannot be pixel-exact.
const COLOR_CHUNKS: [&[u8; 4]; 3] = [b"iCCP", b"cHRM", b"gAMA"];
/// Descriptive PNG chunks that a WebP candidate does not carry over.
const METADATA_CHUNKS: [&[u8; 4]; 5] = [b"eXIf", b"tEXt", b"iTXt", b"zTXt", b"tIME"];

fn png_chunk_names(png: &[u8], wanted: &[&[u8; 4]]) -> Vec<String> {
    let mut found = Vec::new();
    let mut offset = 8_usize;
    while let Some(header) = png.get(offset..offset + 8) {
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let tag = [header[4], header[5], header[6], header[7]];
        if wanted.contains(&&tag) {
            let name = String::from_utf8_lossy(&tag).into_owned();
            if !found.contains(&name) {
                found.push(name);
            }
        }
        let Some(next) = offset.checked_add(length).and_then(|n| n.checked_add(12)) else {
            break;
        };
        offset = next;
    }
    found
}

/// Metadata a PNG → WebP conversion drops, reported instead of discarded silently.
pub(crate) fn dropped_png_metadata(original: &[u8]) -> Vec<String> {
    if original.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_chunk_names(original, &METADATA_CHUNKS)
    } else {
        vec![]
    }
}

/// Straight 8-bit RGBA of a PNG, including RGB under transparent pixels.
fn png_rgba8(png: &[u8], max_pixels: usize) -> Result<(u32, u32, Vec<u8>)> {
    ensure!(
        png_chunk_names(png, &COLOR_CHUNKS).is_empty(),
        "color_profile_not_representable_in_lossless_webp"
    );
    ensure!(
        png_chunk_names(png, &[b"acTL"]).is_empty(),
        "multiple_frames_not_transcoded"
    );
    let mut reader = crate::png_pixels::reader(png, 1 << 30)?;
    let info = reader.info();
    ensure!(
        info.bit_depth != png::BitDepth::Sixteen,
        "sixteen_bit_samples_not_representable_in_webp"
    );
    let (width, height) = (info.width, info.height);
    ensure!(
        (width as usize)
            .checked_mul(height as usize)
            .is_some_and(|n| n > 0 && n <= max_pixels.min(crate::MAX_PIXELS_LIMIT)),
        "decoded_image_exceeds_max_pixels"
    );
    ensure!(
        width <= 16383 && height <= 16383,
        "webp_dimensions_exceed_codec_limit"
    );
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    while let Some(row) = crate::png_pixels::next_rgba_row(&mut reader)? {
        rgba.extend(row.iter().map(|sample| (sample >> 8) as u8));
    }
    ensure!(
        rgba.len() == width as usize * height as usize * 4,
        "truncated_png"
    );
    Ok((width, height, rgba))
}

/// Lossless WebP from an 8-bit sRGB PNG, keeping RGB under transparent pixels.
pub(crate) fn encode_lossless(png: &[u8], max_pixels: usize) -> Result<Vec<u8>> {
    let (width, height, rgba) = png_rgba8(png, max_pixels)?;
    let mut config = webp::WebPConfig::new().map_err(|_| anyhow::anyhow!("webp_config_failed"))?;
    config.lossless = 1;
    config.exact = 1;
    config.quality = 75.0;
    config.method = 4;
    let encoded = webp::Encoder::from_rgba(&rgba, width, height)
        .encode_advanced(&config)
        .map(|bytes| bytes.to_vec())
        .map_err(|e| anyhow::anyhow!("webp_encoding_failed: {e:?}"))?;
    verify_lossless(png, &encoded, max_pixels)?;
    Ok(encoded)
}

/// The invariant behind the "lossless" label: every straight RGBA sample of the
/// PNG, visible or not, equals the decoded WebP sample.
pub(crate) fn verify_lossless(png: &[u8], candidate: &[u8], max_pixels: usize) -> Result<()> {
    let (width, height, expected) = png_rgba8(png, max_pixels)?;
    let features = webp::BitstreamFeatures::new(candidate).context("invalid_webp")?;
    ensure!(!features.has_animation(), "multiple_frames_not_transcoded");
    ensure!(
        features.width() == width && features.height() == height,
        "dimensions_changed"
    );
    let image = webp::Decoder::new(candidate)
        .decode()
        .context("webp_decode_failed")?;
    let same = if image.is_alpha() {
        *image == *expected
    } else {
        image.len() == expected.len() / 4 * 3
            && expected
                .chunks_exact(4)
                .zip(image.chunks_exact(3))
                .all(|(a, b)| a[3] == 255 && a[..3] == *b)
    };
    ensure!(same, "lossless_webp_pixels_changed");
    Ok(())
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
    fn rgba_png(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
        bytes
    }

    #[test]
    fn lossless_webp_keeps_every_sample_including_hidden_rgb() {
        let mut pixels = Vec::new();
        for i in 0..32 * 32 {
            // Fully transparent pixels still carry distinct RGB.
            pixels.extend([
                (i % 251) as u8,
                (i % 13) as u8 * 19,
                7,
                if i % 3 == 0 { 0 } else { 200 },
            ]);
        }
        let png = rgba_png(&pixels, 32, 32);
        let webp = encode_lossless(&png, 4096).unwrap();
        verify_lossless(&png, &webp, 4096).unwrap();
        assert_eq!(&*webp::Decoder::new(&webp).decode().unwrap(), &pixels[..]);
        // A lossy encoding of the same image is not accepted as lossless.
        let lossy = webp::Encoder::from_rgba(&pixels, 32, 32)
            .encode_simple(false, 50.0)
            .unwrap();
        assert!(verify_lossless(&png, &lossy, 4096).is_err());
        assert!(encode_lossless(&png, 1023).is_err());
    }

    #[test]
    fn lossless_webp_refuses_profiles_and_sixteen_bit_sources() {
        let mut deep = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut deep, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Sixteen);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[1; 32])
                .unwrap();
        }
        assert!(encode_lossless(&deep, 4096).is_err());
        let mut gamma = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut gamma, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_source_gamma(png::ScaledFloat::new(0.5));
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[9; 16])
                .unwrap();
        }
        assert_eq!(
            encode_lossless(&gamma, 4096).unwrap_err().to_string(),
            "color_profile_not_representable_in_lossless_webp"
        );
    }

    #[test]
    fn dropped_metadata_is_reported() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder
                .add_text_chunk("Author".into(), "someone".into())
                .unwrap();
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[1, 2, 3, 4])
                .unwrap();
        }
        assert_eq!(dropped_png_metadata(&bytes), ["tEXt"]);
        assert!(dropped_png_metadata(b"RIFF").is_empty());
    }

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
