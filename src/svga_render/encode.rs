//! A rendered frame as a PNG.
use super::Frame;
use anyhow::{Context, Result, ensure};

const BYTES_PER_PIXEL: usize = 4;

pub(crate) fn encode_png(frame: &Frame) -> Result<Vec<u8>> {
    let expected = (frame.width as usize)
        .checked_mul(frame.height as usize)
        .and_then(|pixels| pixels.checked_mul(BYTES_PER_PIXEL));
    ensure!(
        frame.width > 0 && frame.height > 0 && expected == Some(frame.rgba.len()),
        "svga_render_frame_size_mismatch"
    );
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Previews are made on demand and thrown away; speed beats size.
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().context("svga_render_png_header")?;
    writer
        .write_image_data(&frame.rgba)
        .context("svga_render_png_data")?;
    writer.finish().context("svga_render_png_finish")?;
    Ok(bytes)
}
