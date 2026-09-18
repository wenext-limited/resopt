use anyhow::{Context, Result, bail, ensure};
use png::{BitDepth, ColorType, Transformations};
use std::io::Cursor;

type Reader<'a> = png::Reader<Cursor<&'a [u8]>>;

/// Compare two PNGs row by row as straight RGBA16, independent of how each
/// file stores its samples. RGB under fully transparent pixels must match too.
pub(crate) fn ensure_same_rgba(original: &[u8], candidate: &[u8], limit: usize) -> Result<()> {
    let mut original = reader(original, limit)?;
    let mut candidate = reader(candidate, limit)?;
    loop {
        let expected = next_rgba_row(&mut original)?;
        let actual = next_rgba_row(&mut candidate)?;
        ensure!(
            expected == actual,
            "expanded pixels changed; candidate rejected"
        );
        if expected.is_none() {
            return Ok(());
        }
    }
}

pub(crate) fn reader(bytes: &[u8], limit: usize) -> Result<Reader<'_>> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: limit });
    decoder.set_transformations(Transformations::EXPAND);
    Ok(decoder.read_info()?)
}

pub(crate) fn next_rgba_row(reader: &mut Reader<'_>) -> Result<Option<Vec<u16>>> {
    let (color, depth) = reader.output_color_type();
    let Some(row) = reader.next_row()? else {
        return Ok(None);
    };
    let samples: Vec<u16> = match depth {
        BitDepth::Eight => row.data().iter().map(|v| u16::from(*v) * 257).collect(),
        BitDepth::Sixteen => row
            .data()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect(),
        _ => bail!("unexpected expanded bit depth"),
    };
    let channels = color.samples();
    ensure!(samples.len().is_multiple_of(channels), "truncated PNG row");
    let rgba = samples
        .chunks_exact(channels)
        .map(|pixel| match color {
            ColorType::Grayscale => Ok([pixel[0], pixel[0], pixel[0], u16::MAX]),
            ColorType::GrayscaleAlpha => Ok([pixel[0], pixel[0], pixel[0], pixel[1]]),
            ColorType::Rgb => Ok([pixel[0], pixel[1], pixel[2], u16::MAX]),
            ColorType::Rgba => Ok([pixel[0], pixel[1], pixel[2], pixel[3]]),
            ColorType::Indexed => bail!("palette was not expanded"),
        })
        .collect::<Result<Vec<_>>>()
        .context("normalizing PNG row")?;
    Ok(Some(rgba.into_flattened()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 1024 * 1024;

    fn png(color: ColorType, depth: BitDepth, width: u32, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, 1);
            encoder.set_color(color);
            encoder.set_depth(depth);
            if color == ColorType::Indexed {
                encoder.set_palette(vec![255, 0, 0, 0, 0, 255]);
                encoder.set_trns(vec![255, 128]);
            }
            encoder
                .write_header()
                .unwrap()
                .write_image_data(data)
                .unwrap();
        }
        bytes
    }

    #[test]
    fn storage_format_does_not_matter() {
        let rgba = png(
            ColorType::Rgba,
            BitDepth::Eight,
            2,
            &[255, 0, 0, 255, 0, 0, 255, 128],
        );
        let indexed = png(ColorType::Indexed, BitDepth::Eight, 2, &[0, 1]);
        ensure_same_rgba(&rgba, &indexed, LIMIT).unwrap();

        let gray16 = png(
            ColorType::Grayscale,
            BitDepth::Sixteen,
            2,
            &[0, 0, 255, 255],
        );
        let gray1 = png(ColorType::Grayscale, BitDepth::One, 2, &[0b0100_0000]);
        ensure_same_rgba(&gray16, &gray1, LIMIT).unwrap();
    }

    #[test]
    fn sample_changes_are_rejected() {
        let original = png(ColorType::Rgb, BitDepth::Eight, 1, &[10, 20, 30]);
        let shifted = png(ColorType::Rgb, BitDepth::Eight, 1, &[10, 20, 31]);
        assert!(ensure_same_rgba(&original, &shifted, LIMIT).is_err());

        // 0x0101 is exactly 8-bit 1; 0x0102 has no 8-bit equivalent.
        let exact = png(ColorType::Grayscale, BitDepth::Sixteen, 1, &[1, 2]);
        let rounded = png(ColorType::Grayscale, BitDepth::Eight, 1, &[1]);
        assert!(ensure_same_rgba(&exact, &rounded, LIMIT).is_err());
    }

    #[test]
    fn hidden_rgb_and_alpha_are_compared() {
        let original = png(ColorType::Rgba, BitDepth::Eight, 1, &[9, 9, 9, 0]);
        let cleared = png(ColorType::Rgba, BitDepth::Eight, 1, &[0, 0, 0, 0]);
        assert!(ensure_same_rgba(&original, &cleared, LIMIT).is_err());
        let opaque = png(ColorType::Rgba, BitDepth::Eight, 1, &[9, 9, 9, 255]);
        assert!(ensure_same_rgba(&original, &opaque, LIMIT).is_err());
    }

    #[test]
    fn different_heights_are_rejected() {
        let one_row = png(ColorType::Grayscale, BitDepth::Eight, 1, &[7]);
        let mut two_rows = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut two_rows, 1, 2);
            encoder.set_color(ColorType::Grayscale);
            encoder.set_depth(BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[7, 7])
                .unwrap();
        }
        assert!(ensure_same_rgba(&one_row, &two_rows, LIMIT).is_err());
    }
}
