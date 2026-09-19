//! Rasterizing one frame of a prepared scene.
use super::prepare::{Image, Role, Scene, Shape, Sprite, SpriteFrame};
use tiny_skia::{BlendMode, FillRule, FilterQuality, Mask, Paint, Pixmap, PixmapPaint, Transform};

/// Everything a sprite needs to land on a canvas of one size.
struct Target<'a> {
    scene: &'a Scene,
    frame: usize,
    size: (u32, u32),
    /// View box → canvas.
    scale: Transform,
}

/// A run of sprites masked by one matte sprite, drawn off-canvas.
struct Group {
    matte: usize,
    layer: Pixmap,
}

/// Draw `frame` of the scene on a `size` canvas. `None` only when a canvas
/// cannot be allocated. The result is premultiplied.
pub(super) fn frame(
    scene: &Scene,
    frame: usize,
    size: (u32, u32),
    scale: (f32, f32),
) -> Option<Pixmap> {
    let target = Target {
        scene,
        frame,
        size,
        scale: Transform::from_scale(scale.0, scale.1),
    };
    let mut canvas = Pixmap::new(size.0, size.1)?;
    let mut group: Option<Group> = None;
    let visible = scene.sprites.iter().filter(|sprite| {
        sprite.role != Role::Matte && target.frame_of(sprite).is_some_and(SpriteFrame::is_visible)
    });
    for sprite in visible {
        let matte = match sprite.role {
            Role::Masked(index) => Some(index),
            Role::Plain | Role::Matte => None,
        };
        if group.as_ref().map(|group| group.matte) != matte {
            if let Some(finished) = group.take() {
                target.composite(&mut canvas, finished);
            }
            group = match matte {
                Some(matte) => Some(Group {
                    matte,
                    layer: Pixmap::new(size.0, size.1)?,
                }),
                None => None,
            };
        }
        match group.as_mut() {
            Some(group) => target.sprite(&mut group.layer, sprite),
            None => target.sprite(&mut canvas, sprite),
        }
    }
    if let Some(finished) = group {
        target.composite(&mut canvas, finished);
    }
    Some(canvas)
}

impl Target<'_> {
    fn frame_of<'s>(&self, sprite: &'s Sprite) -> Option<&'s SpriteFrame> {
        sprite.frames.get(self.frame)
    }

    /// Keep of the layer only what the matte sprite covers, then paint it.
    fn composite(&self, canvas: &mut Pixmap, group: Group) {
        let Group { matte, mut layer } = group;
        let Some(mut mask) = Pixmap::new(self.size.0, self.size.1) else {
            return;
        };
        if let Some(sprite) = self.scene.sprites.get(matte) {
            self.sprite(&mut mask, sprite);
        }
        let keep = PixmapPaint {
            blend_mode: BlendMode::DestinationIn,
            ..PixmapPaint::default()
        };
        let identity = Transform::identity();
        layer.draw_pixmap(0, 0, mask.as_ref(), &keep, identity, None);
        let over = PixmapPaint::default();
        canvas.draw_pixmap(0, 0, layer.as_ref(), &over, identity, None);
    }

    fn sprite(&self, canvas: &mut Pixmap, sprite: &Sprite) {
        let Some(frame) = self.frame_of(sprite).filter(|frame| frame.is_visible()) else {
            return;
        };
        let matrix = self.scale.pre_concat(frame.transform);
        if !matrix.is_finite() {
            return;
        }
        let clip = match &frame.clip {
            None => None,
            Some(path) => {
                let Some(mut mask) = Mask::new(self.size.0, self.size.1) else {
                    return;
                };
                mask.fill_path(path, FillRule::Winding, true, matrix);
                Some(mask)
            }
        };
        let image = sprite.image.and_then(|index| self.scene.images.get(index));
        if let Some(image) = image {
            bitmap(canvas, image, frame, matrix, clip.as_ref());
        }
        for shape in frame.shapes.iter() {
            self::shape(canvas, shape, frame.alpha, matrix, clip.as_ref());
        }
    }
}

/// The bitmap, stretched from its own size to the frame's layout box.
fn bitmap(
    canvas: &mut Pixmap,
    image: &Image,
    frame: &SpriteFrame,
    matrix: Transform,
    clip: Option<&Mask>,
) {
    let (width, height) = frame.size;
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return;
    }
    let stretch = (
        width / image.pixmap.width() as f32,
        height / image.pixmap.height() as f32,
    );
    let matrix = matrix.pre_scale(stretch.0, stretch.1);
    if !matrix.is_finite() {
        return;
    }
    let paint = PixmapPaint {
        opacity: frame.alpha,
        blend_mode: BlendMode::SourceOver,
        quality: FilterQuality::Bilinear,
    };
    canvas.draw_pixmap(0, 0, image.pixmap.as_ref(), &paint, matrix, clip);
}

fn shape(canvas: &mut Pixmap, shape: &Shape, alpha: f32, matrix: Transform, clip: Option<&Mask>) {
    let matrix = matrix.pre_concat(shape.transform);
    if !matrix.is_finite() {
        return;
    }
    let paint = |color: tiny_skia::Color| {
        let mut faded = color;
        faded.apply_opacity(alpha);
        let mut paint = Paint::default();
        paint.set_color(faded);
        paint
    };
    if let Some(fill) = shape.fill {
        canvas.fill_path(&shape.path, &paint(fill), FillRule::Winding, matrix, clip);
    }
    if let Some((color, stroke)) = &shape.stroke {
        canvas.stroke_path(&shape.path, &paint(*color), stroke, matrix, clip);
    }
}
