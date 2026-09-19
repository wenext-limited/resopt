//! Synthetic files built with the codec itself, checked pixel by pixel.
mod bitmaps;
mod corpus;
mod hostile;
mod shapes;
mod sizing;

use super::{Frame, Renderer};
use svga::movie::{Layout, Params, Sprite, Transform};
use svga::{Compression, Document, Movie};

pub(super) const RED: [u8; 4] = [255, 0, 0, 255];
pub(super) const GREEN: [u8; 4] = [0, 255, 0, 255];
pub(super) const BLUE: [u8; 4] = [0, 0, 255, 255];
pub(super) const CLEAR: [u8; 4] = [0, 0, 0, 0];
/// Side of the square view box most tests draw in, rendered 1:1.
pub(super) const VIEW: u32 = 100;

pub(super) fn solid_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let frame = Frame {
        width,
        height,
        rgba: rgba.repeat((width * height) as usize),
    };
    super::encode_png(&frame).unwrap()
}

pub(super) fn movie(width: f32, height: f32, frames: i32, sprites: Vec<Sprite>) -> Movie {
    Movie {
        version: "2.0.0".into(),
        params: Params {
            view_box_width: width,
            view_box_height: height,
            fps: 30,
            frames,
        },
        sprites,
        ..Movie::default()
    }
}

pub(super) fn file(movie: &Movie, images: &[(&str, &[u8])]) -> Vec<u8> {
    let document = Document::from_movie(movie, images).unwrap();
    document.to_bytes(Compression::Fast).unwrap()
}

pub(super) fn translate(tx: f32, ty: f32) -> Transform {
    Transform {
        tx,
        ty,
        ..Transform::IDENTITY
    }
}

/// An opaque frame with a `width × height` layout box moved to `tx, ty`.
pub(super) fn placed(tx: f32, ty: f32, width: f32, height: f32) -> svga::movie::Frame {
    svga::movie::Frame {
        alpha: 1.0,
        layout: Layout {
            width,
            height,
            ..Layout::default()
        },
        transform: Some(translate(tx, ty)),
        ..svga::movie::Frame::default()
    }
}

pub(super) fn sprite(image_key: &str, frames: Vec<svga::movie::Frame>) -> Sprite {
    Sprite {
        image_key: image_key.into(),
        frames,
        ..Sprite::default()
    }
}

/// Render `frame` of a one-to-one `VIEW`-sized movie.
pub(super) fn render(sprites: Vec<Sprite>, images: &[(&str, &[u8])], frame: usize) -> Frame {
    let frames = sprites.iter().map(|sprite| sprite.frames.len()).max();
    let movie = movie(
        VIEW as f32,
        VIEW as f32,
        frames.unwrap_or(1) as i32,
        sprites,
    );
    let renderer = Renderer::new(&file(&movie, images)).unwrap();
    renderer.render(frame, VIEW).unwrap()
}

pub(super) fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
    let start = ((y * frame.width + x) * 4) as usize;
    frame.rgba[start..start + 4].try_into().unwrap()
}

pub(super) fn is_blank(frame: &Frame) -> bool {
    frame.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0)
}
