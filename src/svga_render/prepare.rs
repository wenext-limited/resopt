//! The decoded movie, reduced once to what drawing a frame needs: decoded
//! images, parsed paths, resolved `keep` shapes and matte links.
use super::image::{self, MAX_TOTAL_PIXELS};
use super::path;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use svga::movie::{Geometry, LineCap, LineJoin, Rgba, ShapeStyle};
use tiny_skia::{Color, Path, Pixmap, Stroke, StrokeDash, Transform};

/// tiny-skia's (and Android's) default, used when a file stores none.
const DEFAULT_MITER_LIMIT: f32 = 4.0;
/// Floors SVGACanvasDrawer.kt applies to a dash and to a gap.
const MIN_DASH: f32 = 1.0;
const MIN_GAP: f32 = 0.1;

pub(super) struct Scene {
    pub images: Vec<Image>,
    pub sprites: Vec<Sprite>,
}

pub(super) struct Image {
    pub pixmap: Pixmap,
    /// Mean alpha of the pixels, `0.0..=1.0`: how much of the bitmap shows.
    pub ink: f64,
}

pub(super) struct Sprite {
    /// Index into [`Scene::images`]; `None` draws shapes only.
    pub image: Option<usize>,
    pub role: Role,
    pub frames: Vec<SpriteFrame>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Plain,
    /// Never painted; only masks the sprites that name it.
    Matte,
    /// Masked by the matte sprite at this index.
    Masked(usize),
}

pub(super) struct SpriteFrame {
    /// `0.0..=1.0`; zero also stands for "cannot be drawn".
    pub alpha: f32,
    pub transform: Transform,
    /// `layout.width × layout.height`, the box a bitmap is stretched to.
    pub size: (f32, f32),
    pub clip: Option<Arc<Path>>,
    pub shapes: Arc<[Shape]>,
}

pub(super) struct Shape {
    pub path: Arc<Path>,
    pub transform: Transform,
    pub fill: Option<Color>,
    pub stroke: Option<(Color, Stroke)>,
}

impl SpriteFrame {
    pub(super) fn is_visible(&self) -> bool {
        self.alpha > 0.0
    }
}

pub(super) fn scene(animation: &svga::Animation) -> Result<Scene> {
    let movie = animation.movie();
    let (images, image_indices) = images(animation)?;
    let mattes: HashMap<&str, usize> = (movie.sprites.iter().enumerate())
        .filter(|(_, sprite)| sprite.is_matte())
        .map(|(index, sprite)| (sprite.image_key.as_str(), index))
        .collect();
    let mut paths = PathCache::default();
    let sprites = (movie.sprites.iter())
        .map(|sprite| Sprite {
            image: image_indices.get(sprite.image_name()).copied().flatten(),
            role: role(sprite, &mattes),
            frames: frames(sprite, &mut paths),
        })
        .collect();
    Ok(Scene { images, sprites })
}

fn role(sprite: &svga::movie::Sprite, mattes: &HashMap<&str, usize>) -> Role {
    if sprite.is_matte() {
        return Role::Matte;
    }
    let matte = mattes.get(sprite.matte_key.as_str());
    matte.map_or(Role::Plain, |index| Role::Masked(*index))
}

/// Image key → index into [`Scene::images`]; `None` when it cannot be drawn.
type ImageIndices<'a> = HashMap<&'a str, Option<usize>>;

/// Decode each image a sprite draws, once, within the file's pixel budget.
fn images(animation: &svga::Animation) -> Result<(Vec<Image>, ImageIndices<'_>)> {
    let mut images = Vec::new();
    let mut indices = HashMap::new();
    let mut budget = MAX_TOTAL_PIXELS;
    for sprite in &animation.movie().sprites {
        let key = sprite.image_name();
        if key.is_empty() || indices.contains_key(key) {
            continue;
        }
        let Some(bytes) = animation.image(key) else {
            indices.insert(key, None);
            continue;
        };
        let decoded = image::decode(bytes, budget)?;
        budget = budget.saturating_sub(decoded.pixels);
        let index = decoded.pixmap.map(|pixmap| {
            images.push(Image {
                ink: image::ink(&pixmap),
                pixmap,
            });
            images.len() - 1
        });
        indices.insert(key, index);
    }
    Ok((images, indices))
}

/// Path strings repeat across frames; each distinct one is parsed once.
#[derive(Default)]
struct PathCache<'a> {
    parsed: HashMap<&'a str, Option<Arc<Path>>>,
}

impl<'a> PathCache<'a> {
    fn get(&mut self, d: &'a str) -> Option<Arc<Path>> {
        let parsed = self.parsed.entry(d);
        parsed
            .or_insert_with(|| path::parse(d).map(Arc::new))
            .clone()
    }
}

fn frames<'a>(sprite: &'a svga::movie::Sprite, paths: &mut PathCache<'a>) -> Vec<SpriteFrame> {
    let no_shapes: Arc<[Shape]> = Arc::from(Vec::new());
    let resolved = sprite.frames.iter().scan(no_shapes, |previous, frame| {
        // Players look at the first shape only to decide that a frame keeps.
        let keeps = matches!(frame.shapes.first(), Some(shape) if shape.geometry == Geometry::Keep);
        if !keeps {
            let shapes = frame
                .shapes
                .iter()
                .filter_map(|shape| self::shape(shape, paths));
            *previous = shapes.collect();
        }
        Some(sprite_frame(frame, previous.clone(), paths))
    });
    resolved.collect()
}

fn sprite_frame<'a>(
    frame: &'a svga::movie::Frame,
    shapes: Arc<[Shape]>,
    paths: &mut PathCache<'a>,
) -> SpriteFrame {
    let transform = transform(frame.transform);
    let clip = (!frame.clip_path.is_empty()).then(|| paths.get(&frame.clip_path));
    // A clip without geometry clips everything, as in the players.
    let drawable = transform.is_some() && !matches!(clip, Some(None));
    let alpha = if drawable && frame.alpha.is_finite() {
        frame.alpha.clamp(0.0, 1.0)
    } else {
        0.0
    };
    SpriteFrame {
        alpha,
        transform: transform.unwrap_or_default(),
        size: (frame.layout.width, frame.layout.height),
        clip: clip.flatten(),
        shapes,
    }
}

/// `None` for a matrix with a non-finite entry; the identity when absent.
fn transform(stored: Option<svga::movie::Transform>) -> Option<Transform> {
    let Some(matrix) = stored else {
        return Some(Transform::identity());
    };
    let transform =
        Transform::from_row(matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty);
    transform.is_finite().then_some(transform)
}

fn shape<'a>(shape: &'a svga::movie::Shape, paths: &mut PathCache<'a>) -> Option<Shape> {
    let styles = shape.styles.as_ref()?;
    let fill = styles.fill.and_then(color);
    let stroke = styles.stroke.and_then(color).zip(stroke(styles));
    if fill.is_none() && stroke.is_none() {
        return None;
    }
    let path = match &shape.geometry {
        Geometry::Path { d } => paths.get(d)?,
        Geometry::Rect {
            x,
            y,
            width,
            height,
            corner_radius,
        } => Arc::new(path::rect(*x, *y, *width, *height, *corner_radius)?),
        Geometry::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
        } => Arc::new(path::ellipse(*x, *y, *radius_x, *radius_y)?),
        Geometry::Keep => return None,
    };
    Some(Shape {
        path,
        transform: transform(shape.transform)?,
        fill,
        stroke,
    })
}

/// `None` for a colour that paints nothing.
fn color(rgba: Rgba) -> Option<Color> {
    let unit = |value: f32| value.is_finite().then(|| value.clamp(0.0, 1.0));
    let color = Color::from_rgba(unit(rgba.r)?, unit(rgba.g)?, unit(rgba.b)?, unit(rgba.a)?)?;
    (color.alpha() > 0.0).then_some(color)
}

fn stroke(styles: &ShapeStyle) -> Option<Stroke> {
    let width = styles.stroke_width;
    if !(width.is_finite() && width > 0.0) {
        return None;
    }
    let miter = styles.miter_limit;
    Some(Stroke {
        width,
        miter_limit: if miter.is_finite() && miter > 0.0 {
            miter
        } else {
            DEFAULT_MITER_LIMIT
        },
        line_cap: match styles.line_cap {
            LineCap::Butt => tiny_skia::LineCap::Butt,
            LineCap::Round => tiny_skia::LineCap::Round,
            LineCap::Square => tiny_skia::LineCap::Square,
        },
        line_join: match styles.line_join {
            LineJoin::Miter => tiny_skia::LineJoin::Miter,
            LineJoin::Round => tiny_skia::LineJoin::Round,
            LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
        },
        dash: dash(styles.line_dash),
    })
}

/// `[dash, gap, offset]`; off unless a dash or a gap is set.
fn dash([dash, gap, offset]: [f32; 3]) -> Option<StrokeDash> {
    if !(dash > 0.0 || gap > 0.0) {
        return None;
    }
    let offset = if offset.is_finite() { offset } else { 0.0 };
    StrokeDash::new(vec![dash.max(MIN_DASH), gap.max(MIN_GAP)], offset)
}
