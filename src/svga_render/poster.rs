//! Which frame stands for the whole animation in a thumbnail.
use super::prepare::{Role, Scene, Sprite, SpriteFrame};

/// How much a frame shows: drawn sprites first, then the bitmap area they
/// cover. The second part tells apart the frames of a bitmap sequence, where
/// every frame draws exactly one sprite and the first is often empty.
#[derive(Clone, Copy, PartialEq, PartialOrd, Default)]
struct Score {
    sprites: usize,
    ink: f64,
}

/// The frame with the highest score; the earliest of equals.
pub(super) fn frame(scene: &Scene, frame_count: usize) -> usize {
    let scores = (0..frame_count).map(|index| score(scene, index));
    let best = scores.enumerate().reduce(|best, next| {
        // Strictly greater, so the earliest of equals wins.
        if next.1 > best.1 { next } else { best }
    });
    best.map_or(0, |(index, _)| index)
}

fn score(scene: &Scene, index: usize) -> Score {
    let drawn = scene.sprites.iter().filter_map(|sprite| {
        let frame = sprite.frames.get(index)?;
        let counts = sprite.role != Role::Matte
            && frame.is_visible()
            && (sprite.image.is_some() || !frame.shapes.is_empty());
        counts.then(|| ink(scene, sprite, frame))
    });
    drawn.fold(Score::default(), |total, ink| Score {
        sprites: total.sprites + 1,
        ink: total.ink + ink,
    })
}

/// View-box area the sprite's bitmap inks on this frame, faded by its alpha.
fn ink(scene: &Scene, sprite: &Sprite, frame: &SpriteFrame) -> f64 {
    let Some(image) = sprite.image.and_then(|index| scene.images.get(index)) else {
        return 0.0;
    };
    let matrix = frame.transform;
    let stretch = f64::from(matrix.sx * matrix.sy - matrix.kx * matrix.ky).abs();
    let area = f64::from(frame.size.0) * f64::from(frame.size.1) * stretch;
    let ink = area * image.ink * f64::from(frame.alpha);
    if ink.is_finite() { ink.max(0.0) } else { 0.0 }
}
