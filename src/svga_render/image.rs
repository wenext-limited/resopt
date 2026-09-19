//! Embedded PNGs → premultiplied pixmaps, within a pixel budget.
use anyhow::{Result, bail};
use png::{BitDepth, ColorType, Transformations};
use std::io::Cursor;
use tiny_skia::{ColorU8, IntSize, Pixmap};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Longest side of one embedded image.
pub(super) const MAX_IMAGE_SIDE: u32 = 4096;
/// Decoded pixels across all images of one file (256 MiB of RGBA).
pub(super) const MAX_TOTAL_PIXELS: u64 = 64 * 1024 * 1024;
const BYTES_PER_PIXEL: usize = 4;
/// Memory the PNG decoder may use for one image: the largest accepted image
/// plus room for its ancillary chunks.
const DECODER_BYTES: usize = 96 * 1024 * 1024;

pub(super) struct Decoded {
    /// Pixels the header claimed, charged to the budget even when the data
    /// turns out to be broken: the buffer for them was allocated.
    pub pixels: u64,
    /// `None` when this is not a PNG, or a broken one: the sprite is skipped.
    pub pixmap: Option<Pixmap>,
}

const UNUSABLE: Decoded = Decoded {
    pixels: 0,
    pixmap: None,
};

/// Decode one image value. `budget` is how many pixels may still be decoded;
/// exceeding it, or the per-image cap, refuses the whole file.
pub(super) fn decode(bytes: &[u8], budget: u64) -> Result<Decoded> {
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Ok(UNUSABLE);
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: DECODER_BYTES,
    });
    decoder.set_transformations(
        Transformations::EXPAND | Transformations::STRIP_16 | Transformations::ALPHA,
    );
    let Ok(mut reader) = decoder.read_info() else {
        return Ok(UNUSABLE);
    };
    let (width, height) = (reader.info().width, reader.info().height);
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        bail!("svga_render_image_too_large");
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > budget {
        bail!("svga_render_image_pixels_exceed_limit");
    }
    let pixmap = reader.output_buffer_size().and_then(|size| {
        let mut buffer = vec![0; size];
        let info = reader.next_frame(&mut buffer).ok()?;
        pixmap(&info, buffer.get(..info.buffer_size())?)
    });
    Ok(Decoded { pixels, pixmap })
}

/// The decoder was asked for 8-bit samples with alpha; anything else is a
/// frame this renderer does not draw.
fn pixmap(info: &png::OutputInfo, samples: &[u8]) -> Option<Pixmap> {
    if info.bit_depth != BitDepth::Eight {
        return None;
    }
    let rgba = match info.color_type {
        ColorType::Rgba => premultiplied(samples.as_chunks::<4>().0.iter().copied()),
        ColorType::GrayscaleAlpha => premultiplied(
            (samples.as_chunks::<2>().0.iter()).map(|[gray, alpha]| [*gray, *gray, *gray, *alpha]),
        ),
        _ => return None,
    };
    IntSize::from_wh(info.width, info.height)
        .filter(|_| rgba.len() == pixel_bytes(info.width, info.height))
        .and_then(|size| Pixmap::from_vec(rgba, size))
}

/// Mean alpha of a pixmap, `0.0..=1.0`.
pub(super) fn ink(pixmap: &Pixmap) -> f64 {
    let pixels = pixmap.pixels();
    let total: u64 = pixels.iter().map(|pixel| u64::from(pixel.alpha())).sum();
    total as f64 / (pixels.len().max(1) as f64 * f64::from(u8::MAX))
}

fn pixel_bytes(width: u32, height: u32) -> usize {
    (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(BYTES_PER_PIXEL)
}

fn premultiplied(pixels: impl Iterator<Item = [u8; 4]>) -> Vec<u8> {
    pixels
        .flat_map(|[red, green, blue, alpha]| {
            let color = ColorU8::from_rgba(red, green, blue, alpha).premultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}
