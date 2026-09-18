#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub decoder_type: String,
    pub width: usize,
    pub height: usize,
    pub frames: usize,
    pub bits_per_component: usize,
    pub orientation: i64,
    pub transparent_pixels: usize,
    pub has_transparent_pixels: bool,
}

pub(crate) struct Decoded {
    pub info: ImageInfo,
    /// Premultiplied RGBA, rendered into a common sRGB float context.
    pub pixels: Vec<f32>,
}

/// Default decoded-pixel cap: a 256 MiB float buffer per decoded image.
pub const DEFAULT_MAX_PIXELS: usize = 16 * 1024 * 1024;
/// Hard ceiling for a configured pixel cap (1 GiB float buffer per image).
pub const MAX_PIXELS_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDifference {
    pub rgb_mae_255: f64,
    pub psnr_db: Option<f64>,
    pub max_alpha_error: f32,
    /// Worst SSIMULACRA2 score over black, white and gray backdrops
    /// (100 = identical). Absent in reports written before it was measured.
    #[serde(default)]
    pub ssimulacra2: Option<f64>,
}

pub(crate) fn compare(a: &Decoded, b: &Decoded) -> Result<ImageDifference> {
    ensure!(
        a.info.width == b.info.width && a.info.height == b.info.height,
        "dimensions_changed"
    );
    ensure!(
        a.info.orientation == b.info.orientation,
        "orientation_changed"
    );
    ensure!(
        a.info.frames == 1 && b.info.frames == 1 && !a.pixels.is_empty(),
        "multiple_frames"
    );
    ensure!(a.pixels.len() == b.pixels.len(), "pixel_buffer_mismatch");
    let mut absolute = 0.0_f64;
    let mut squared = 0.0_f64;
    let mut alpha = 0.0_f32;
    for (a, b) in a
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0.iter())
    {
        for channel in 0..3 {
            let delta = f64::from(a[channel] - b[channel]);
            absolute += delta.abs();
            squared += delta * delta;
        }
        alpha = alpha.max((a[3] - b[3]).abs());
    }
    let channels = (a.pixels.len() / 4 * 3) as f64;
    let mse = squared / channels;
    Ok(ImageDifference {
        rgb_mae_255: absolute / channels * 255.0,
        psnr_db: (mse > 0.0).then(|| -10.0 * mse.log10()),
        max_alpha_error: alpha,
        ssimulacra2: Some(crate::quality::ssimulacra2(a, b)?),
    })
}

pub(crate) fn preview(image: &Decoded) -> Result<Vec<u8>> {
    let (display_width, display_height) = if (5..=8).contains(&image.info.orientation) {
        (image.info.height, image.info.width)
    } else {
        (image.info.width, image.info.height)
    };
    let scale = (256.0 / display_width.max(display_height) as f64).min(1.0);
    let width = (display_width as f64 * scale).round().max(1.0) as usize;
    let height = (display_height as f64 * scale).round().max(1.0) as usize;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let dx = x * display_width / width;
            let dy = y * display_height / height;
            let w = image.info.width;
            let h = image.info.height;
            let (sx, sy) = match image.info.orientation {
                2 => (w - 1 - dx, dy),
                3 => (w - 1 - dx, h - 1 - dy),
                4 => (dx, h - 1 - dy),
                5 => (dy, dx),
                6 => (dy, h - 1 - dx),
                7 => (w - 1 - dy, h - 1 - dx),
                8 => (w - 1 - dy, dx),
                _ => (dx, dy),
            };
            let index = (sy * image.info.width + sx) * 4;
            let pixel = &image.pixels[index..index + 4];
            let alpha = pixel[3].clamp(0.0, 1.0);
            for value in &pixel[..3] {
                bytes.push(if alpha == 0.0 {
                    0
                } else {
                    (value / alpha * 255.0).round().clamp(0.0, 255.0) as u8
                });
            }
            bytes.push((alpha * 255.0).round() as u8);
        }
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        encoder.write_header()?.write_image_data(&bytes)?;
    }
    Ok(output)
}

pub fn image_backend_available() -> bool {
    cfg!(target_os = "macos")
}

pub(crate) fn check_encoders() -> Result<()> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(&vec![128; 64 * 64 * 3])?;
    }
    for format in ["jpeg", "heic"] {
        let encoded = encode(&bytes, format, 85).map_err(|error| anyhow::anyhow!("{format} encoder unavailable: {error}; image analysis requires macOS ImageIO access (restrictive sandboxes can block HEIC); use --probe-only for detection without encoding"))?;
        decode(&encoded, DEFAULT_MAX_PIXELS)?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn decode(_: &[u8], _: usize) -> Result<Decoded> {
    anyhow::bail!("image_analysis_requires_macos_imageio")
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn encode(_: &[u8], _: &str, _: u8) -> Result<Vec<u8>> {
    anyhow::bail!("image_encoding_requires_macos_imageio")
}

#[cfg(target_os = "macos")]
mod apple {
    use super::*;
    use objc2_core_foundation::{
        CFData, CFDictionary, CFMutableData, CFNumber, CFRetained, CFString, CFType, CGPoint,
        CGRect, CGSize,
    };
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, kCGColorSpaceSRGB,
    };
    use objc2_image_io::{
        CGImageDestination, CGImageSource, kCGImageDestinationLossyCompressionQuality,
        kCGImagePropertyOrientation,
    };

    fn source(bytes: &[u8]) -> Result<CFRetained<CGImageSource>> {
        ensure!(bytes.len() <= 64 * 1024 * 1024, "input_exceeds_64_mib");
        let data = CFData::from_bytes(bytes);
        // SAFETY: No options dictionary; CFData owns copied input bytes and the
        // retained source keeps the backing data alive for decoding.
        unsafe { CGImageSource::with_data(&data, None) }.context("imageio_cannot_read_image")
    }

    pub(crate) fn decode(bytes: &[u8], max_pixels: usize) -> Result<Decoded> {
        let source = source(bytes)?;
        // SAFETY: Retained source; no mutable aliases or incorrectly typed options.
        let (frames, decoder_type, image, orientation) = unsafe {
            let frames = source.count();
            ensure!(frames > 0, "image_has_no_frames");
            let image = source
                .image_at_index(0, None)
                .context("imageio_decode_failed")?;
            let properties = source.properties_at_index(0, None);
            let orientation = properties
                .as_ref()
                .and_then(|dictionary| {
                    dictionary
                        .cast_unchecked::<CFString, CFType>()
                        .get(kCGImagePropertyOrientation)
                })
                .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
                .unwrap_or(1);
            (
                frames,
                source
                    .r#type()
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                image,
                orientation,
            )
        };
        let width = CGImage::width(Some(&image));
        let height = CGImage::height(Some(&image));
        let pixel_count = width
            .checked_mul(height)
            .context("image_dimensions_overflow")?;
        ensure!(
            width > 0 && height > 0 && pixel_count <= max_pixels.min(MAX_PIXELS_LIMIT),
            "decoded_image_exceeds_max_pixels"
        );
        let length = pixel_count * 4;
        let mut pixels = vec![0_f32; length];
        // SAFETY: Static color space identifier is provided by CoreGraphics.
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
            .context("srgb_unavailable")?;
        // SAFETY: `pixels` is initialized, 32-bit aligned and large enough for
        // width*height*4 floats. It does not move or get accessed while the context
        // uses its pointer. Drop context before reading the buffer. Flags specify
        // float32 little-endian RGBA with premultiplied-last alpha on macOS.
        let context = unsafe {
            CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                width,
                height,
                32,
                width * 16,
                Some(&space),
                1 | (1 << 8) | (2 << 12),
            )
        }
        .context("float_bitmap_context_unavailable")?;
        CGContext::draw_image(
            Some(&context),
            CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: width as f64,
                    height: height as f64,
                },
            },
            Some(&image),
        );
        drop(context);
        ensure!(
            pixels.iter().all(|v| v.is_finite()),
            "non_finite_decoded_samples"
        );
        let transparent_pixels = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] < 1.0)
            .count();
        Ok(Decoded {
            info: ImageInfo {
                decoder_type,
                width,
                height,
                frames,
                bits_per_component: CGImage::bits_per_component(Some(&image)),
                orientation,
                transparent_pixels,
                has_transparent_pixels: transparent_pixels > 0,
            },
            pixels,
        })
    }

    pub(crate) fn encode(bytes: &[u8], format: &str, quality: u8) -> Result<Vec<u8>> {
        ensure!(
            matches!(format, "jpeg" | "heic") && (1..=100).contains(&quality),
            "invalid_encoding_options"
        );
        let source = source(bytes)?;
        // SAFETY: All objects are retained for the duration of encoding. The
        // dictionary contains the documented CFString quality key and CFNumber
        // value in 0..1. Destination owns no Rust buffer pointers.
        unsafe {
            ensure!(source.count() == 1, "multiple_frames_not_transcoded");
            let output = CFMutableData::new(None, 0).context("cannot_allocate_encoded_buffer")?;
            let type_id = CFString::from_str(if format == "jpeg" {
                "public.jpeg"
            } else {
                "public.heic"
            });
            let destination = CGImageDestination::with_data(&output, &type_id, 1, None)
                .context("requested_encoder_unavailable")?;
            let quality = CFNumber::new_f64(f64::from(quality) / 100.0);
            let options = CFDictionary::<CFString, CFType>::from_slices(
                &[kCGImageDestinationLossyCompressionQuality],
                &[quality.as_ref()],
            );
            destination.add_image_from_source(&source, 0, Some(options.as_opaque()));
            ensure!(destination.finalize(), "imageio_encoding_failed");
            drop(destination);
            Ok(output.to_vec())
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use apple::{decode, encode};

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn png(alpha: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 128, 128);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut data = Vec::new();
            for i in 0..128 * 128 {
                data.extend_from_slice(&[
                    (i % 256) as u8,
                    100,
                    75,
                    if i % 2 == 0 { alpha } else { 255 },
                ]);
            }
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&data)
                .unwrap();
        }
        bytes
    }

    #[test]
    fn opaque_alpha_channel_is_not_transparency() {
        let decoded = decode(&png(255), DEFAULT_MAX_PIXELS).unwrap();
        assert!(!decoded.info.has_transparent_pixels);
        assert_eq!(decoded.info.transparent_pixels, 0);
    }

    #[test]
    fn sixteen_bit_near_opaque_alpha_is_still_transparency() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Sixteen);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 0, 0, 0, 0, 0, 255, 254])
                .unwrap();
        }
        let image = decode(&bytes, DEFAULT_MAX_PIXELS).unwrap();
        assert!(image.info.has_transparent_pixels);
        assert_eq!(image.info.transparent_pixels, 1);
    }

    #[test]
    fn transparent_heic_retains_alpha_and_can_be_decoded() {
        let bytes = png(128);
        let original = decode(&bytes, DEFAULT_MAX_PIXELS).unwrap();
        assert_eq!(original.info.transparent_pixels, 8192);
        let heic = encode(&bytes, "heic", 85).unwrap();
        let decoded = decode(&heic, DEFAULT_MAX_PIXELS).unwrap();
        assert!(decoded.info.decoder_type.contains("heic"));
        assert!(decoded.info.has_transparent_pixels);
        let difference = compare(&original, &decoded).unwrap();
        assert!(
            difference.max_alpha_error <= 1.0 / 255.0 + 0.000001,
            "{difference:?}"
        );
    }

    #[test]
    fn opaque_image_can_compare_jpeg_and_heic() {
        let bytes = png(255);
        let original = decode(&bytes, DEFAULT_MAX_PIXELS).unwrap();
        for format in ["jpeg", "heic"] {
            let encoded = encode(&bytes, format, 75).unwrap();
            let decoded = decode(&encoded, DEFAULT_MAX_PIXELS).unwrap();
            let difference = compare(&original, &decoded).unwrap();
            assert!(difference.rgb_mae_255.is_finite());
            assert!(difference.max_alpha_error <= 0.000001);
        }
    }
}
