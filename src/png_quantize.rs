//! Lossy PNG: palette quantization to at most 256 colours.
//!
//! The result is an ordinary indexed PNG, so it needs no new decoder anywhere
//! and keeps the file's format, name and references. It is a *lossy* candidate:
//! it is scored, thresholded and approved exactly like JPEG, HEIC and WebP.
use anyhow::{Context, Result, ensure};
use exoquant::{Color, convert_to_indexed, ditherer, optimizer};
use std::collections::HashMap;

/// Chunks that describe how samples are interpreted or displayed. They are
/// carried over unchanged so the quantized file renders in the same colour
/// space and at the same physical size.
const CARRIED_CHUNKS: [&[u8; 4]; 6] = [b"iCCP", b"sRGB", b"gAMA", b"cHRM", b"pHYs", b"eXIf"];

/// Palette size for an encoder quality in 1..=100.
pub(crate) fn colors_for_quality(quality: u8) -> usize {
    match quality {
        90..=100 => 256,
        80..=89 => 128,
        70..=79 => 64,
        50..=69 => 32,
        _ => 16,
    }
}

fn carried_chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut found = Vec::new();
    let mut offset = 8_usize;
    while let Some(header) = png.get(offset..offset + 8) {
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let kind = [header[4], header[5], header[6], header[7]];
        let Some(end) = offset.checked_add(length).and_then(|n| n.checked_add(12)) else {
            break;
        };
        if kind == *b"IDAT" || end > png.len() {
            break;
        }
        if CARRIED_CHUNKS.contains(&&kind) {
            found.push((kind, png[offset + 8..end - 4].to_vec()));
        }
        offset = end;
    }
    found
}

/// Squared distance between a pixel and a palette entry. Alpha differences
/// are weighted up, and colour differences matter less the more transparent
/// the pixel is, so translucent edges keep their shape.
fn distance(pixel: [u8; 4], entry: [u8; 4]) -> f32 {
    let alpha = f32::from(pixel[3]) - f32::from(entry[3]);
    let visible = (f32::from(pixel[3].max(entry[3])) / 255.0).max(1.0 / 255.0);
    let colour: f32 = (0..3)
        .map(|c| (f32::from(pixel[c]) - f32::from(entry[c])).powi(2))
        .sum();
    alpha * alpha * 4.0 + colour * visible * visible
}

/// Nearest palette entry per pixel. No dithering: on real app artwork error
/// diffusion lowered perceptual scores, enlarged files and, when applied to
/// alpha, turned soft edges into speckle.
fn remap(rgba: &[[u8; 4]], palette: &[[u8; 4]]) -> Vec<u8> {
    let mut known: HashMap<[u8; 4], u8> = HashMap::new();
    rgba.iter()
        .map(|pixel| {
            *known.entry(*pixel).or_insert_with(|| {
                let mut best = (0_usize, f32::INFINITY);
                for (index, entry) in palette.iter().enumerate() {
                    let d = distance(*pixel, *entry);
                    if d < best.1 {
                        best = (index, d);
                    }
                }
                best.0 as u8
            })
        })
        .collect()
}

/// K-means averages can leave an entry at alpha 254 or 1 even though every
/// pixel mapped to it is fully opaque or fully transparent. Snap those entries,
/// so an opaque image stays opaque and hard transparency stays exact.
fn snap_alpha(mut palette: Vec<[u8; 4]>, pixels: &[[u8; 4]], indexes: &[u8]) -> Vec<[u8; 4]> {
    let mut range = vec![(u8::MAX, u8::MIN); palette.len()];
    for (pixel, index) in pixels.iter().zip(indexes) {
        let entry = &mut range[usize::from(*index)];
        *entry = (entry.0.min(pixel[3]), entry.1.max(pixel[3]));
    }
    for (entry, (lowest, highest)) in palette.iter_mut().zip(range) {
        if lowest == highest && (lowest == 0 || lowest == u8::MAX) {
            entry[3] = lowest;
        }
    }
    palette
}

/// Quantize a PNG to an indexed PNG with at most `colors` palette entries.
pub(crate) fn quantize(png_bytes: &[u8], colors: usize, max_pixels: usize) -> Result<Vec<u8>> {
    ensure!((2..=256).contains(&colors), "invalid_palette_size");
    let mut reader = crate::png_pixels::reader(png_bytes, 1 << 30)?;
    let info = reader.info();
    ensure!(
        info.animation_control.is_none(),
        "multiple_frames_not_transcoded"
    );
    let (width, height) = (info.width, info.height);
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .filter(|n| *n > 0 && *n <= max_pixels.min(crate::MAX_PIXELS_LIMIT))
        .context("decoded_image_exceeds_max_pixels")?;
    let mut pixels: Vec<[u8; 4]> = Vec::with_capacity(pixel_count);
    while let Some(row) = crate::png_pixels::next_rgba_row(&mut reader)? {
        // Rows are RGBA16; reducing 16-bit sources is part of what makes this
        // candidate lossy.
        pixels.extend(
            row.as_chunks::<4>()
                .0
                .iter()
                .map(|p| p.map(|sample| (sample >> 8) as u8)),
        );
    }
    ensure!(pixels.len() == pixel_count, "truncated_png");
    let colours: Vec<Color> = pixels
        .iter()
        .map(|p| Color {
            r: p[0],
            g: p[1],
            b: p[2],
            a: p[3],
        })
        .collect();
    let (palette, _) = convert_to_indexed(
        &colours,
        width as usize,
        colors,
        &optimizer::KMeans,
        &ditherer::None,
    );
    let palette: Vec<[u8; 4]> = palette.iter().map(|c| [c.r, c.g, c.b, c.a]).collect();
    ensure!(!palette.is_empty(), "quantizer_produced_no_palette");
    let indexes = remap(&pixels, &palette);
    let palette = snap_alpha(palette, &pixels, &indexes);
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(
            palette
                .iter()
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect::<Vec<u8>>(),
        );
        if palette.iter().any(|c| c[3] < 255) {
            encoder.set_trns(palette.iter().map(|c| c[3]).collect::<Vec<u8>>());
        }
        let mut writer = encoder.write_header()?;
        for (kind, data) in carried_chunks(png_bytes) {
            writer.write_chunk(png::chunk::ChunkType(kind), &data)?;
        }
        writer.write_image_data(&indexes)?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32, deep: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(if deep {
            png::BitDepth::Sixteen
        } else {
            png::BitDepth::Eight
        });
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: 2835,
            yppu: 2835,
            unit: png::Unit::Meter,
        }));
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let pixel = [
                    (x * 255 / width) as u8,
                    (y * 255 / height) as u8,
                    90,
                    if x < 4 { 0 } else { 255 },
                ];
                for sample in pixel {
                    data.push(sample);
                    if deep {
                        data.push(sample);
                    }
                }
            }
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
        bytes
    }

    fn describe(bytes: &[u8]) -> (png::ColorType, u32, u32, usize, bool) {
        let reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let info = reader.info();
        (
            info.color_type,
            info.width,
            info.height,
            info.palette.as_ref().map_or(0, |p| p.len() / 3),
            info.pixel_dims.is_some(),
        )
    }

    #[test]
    fn output_is_an_indexed_png_with_the_same_size_and_physical_dimensions() {
        let source = gradient(64, 48, false);
        for colors in [256, 32, 2] {
            let (kind, width, height, palette, physical) =
                describe(&quantize(&source, colors, 1 << 20).unwrap());
            assert_eq!((kind, width, height), (png::ColorType::Indexed, 64, 48));
            assert!((1..=colors).contains(&palette), "{palette} of {colors}");
            assert!(physical, "pHYs must be carried over");
        }
    }

    #[test]
    fn transparency_is_matched_exactly_when_the_palette_allows_it() {
        let quantized = quantize(&gradient(64, 48, false), 256, 1 << 20).unwrap();
        let mut reader = crate::png_pixels::reader(&quantized, 1 << 24).unwrap();
        while let Some(row) = crate::png_pixels::next_rgba_row(&mut reader).unwrap() {
            for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
                assert_eq!(pixel[3] == 0, x < 4, "alpha at x={x}");
            }
        }
    }

    #[test]
    fn sixteen_bit_sources_are_accepted_and_limits_and_animation_are_enforced() {
        let deep = gradient(16, 16, true);
        assert_eq!(
            describe(&quantize(&deep, 64, 1 << 20).unwrap()).0,
            png::ColorType::Indexed
        );
        assert!(quantize(&deep, 64, 255).is_err());
        assert!(quantize(&deep, 1, 1 << 20).is_err());
        assert!(quantize(&deep, 257, 1 << 20).is_err());
        assert!(quantize(b"not a png", 64, 1 << 20).is_err());
    }

    #[test]
    fn an_opaque_image_stays_fully_opaque() {
        let mut source = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut source, 96, 96);
            encoder.set_color(png::ColorType::Rgba);
            let data: Vec<u8> = (0..96 * 96_u32)
                .flat_map(|p| {
                    [
                        (p % 96 * 2) as u8,
                        (p / 96 * 2) as u8,
                        (p % 7 * 30) as u8,
                        255,
                    ]
                })
                .collect();
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&data)
                .unwrap();
        }
        for colors in [16, 64, 256] {
            let quantized = quantize(&source, colors, 1 << 20).unwrap();
            let mut reader = crate::png_pixels::reader(&quantized, 1 << 24).unwrap();
            while let Some(row) = crate::png_pixels::next_rgba_row(&mut reader).unwrap() {
                assert!(
                    row.as_chunks::<4>().0.iter().all(|p| p[3] == u16::MAX),
                    "{colors} colours"
                );
            }
        }
    }

    #[test]
    fn quality_maps_to_a_palette_size() {
        assert_eq!(colors_for_quality(95), 256);
        assert_eq!(colors_for_quality(85), 128);
        assert_eq!(colors_for_quality(75), 64);
        assert_eq!(colors_for_quality(1), 16);
    }
}
