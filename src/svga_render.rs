//! SVGA frame renderer: file bytes and a frame index in, an RGBA bitmap out.
//!
//! Self-contained on purpose (only `svga`, `tiny-skia`, `svgtypes`, `png` and
//! `anyhow`), so it can move into a crate of its own. Input is untrusted:
//! every size is capped, a sprite that cannot be drawn is skipped rather than
//! failing the frame, and nothing here panics on hostile data.
//!
//! # Semantics (from reference players)
//!
//! Read from SVGAPlayer-Android `drawer/SVGACanvasDrawer.kt`,
//! `drawer/SGVADrawer.kt`, `entities/SVGAPathEntity.kt`,
//! `entities/SVGAVideoSpriteEntity.kt`; SVGAPlayer-Web `src/Canvas/renderer.js`,
//! `src/frameEntity.js`; SVGAPlayer-iOS `Source/SVGAPlayer.m`.
//!
//! - **Scaling**: the view box is scaled onto the canvas and every sprite is
//!   drawn with `canvas scale · frame transform` (`shareFrameMatrix` in
//!   SVGACanvasDrawer.kt). The output keeps the view box's aspect ratio, so
//!   the scale is uniform up to pixel rounding.
//! - **Sprites** paint in list order. A sprite is skipped on a frame it has no
//!   entry for, or whose `alpha <= 0` (`requestFrameSprites` in SGVADrawer.kt;
//!   the web player's cut-off is `alpha < 0.05`, the native players' is 0).
//! - **Bitmaps**: drawn at the origin of the frame transform, stretched to
//!   `layout.width × layout.height` (`frameMatrix.preScale(layout.width /
//!   bitmap.width, ..)` in `drawImage`), with the frame alpha. `layout.x/y`
//!   is not read by any canvas player. The image of `imageKey` is looked up
//!   without a `.matte` suffix.
//! - **clipPath** is a path in the sprite's own coordinates: it is mapped by
//!   the same frame matrix (`path.transform(frameMatrix); canvas.clipPath`)
//!   and clips the bitmap and the shapes. A clip that yields no geometry clips
//!   everything, as an empty `Path`/`ctx.clip()` does.
//! - **Shapes** are drawn after the bitmap with `frame matrix · shape
//!   transform` (`shapeMatrix.postConcat(frameMatrix)`; `ctx.transform` twice
//!   in renderer.js). Fill, then stroke, each with the frame alpha, non-zero
//!   winding. A stroke needs `strokeWidth > 0`; the width scales with the
//!   whole matrix as on canvas. `lineCap`/`lineJoin` map by name,
//!   `miterLimit` as is, `lineDash` is `[dash, gap, offset]` with the Android
//!   floors (dash ≥ 1, gap ≥ 0.1) and is off when both are 0. Rects clamp the
//!   corner radius to half the shorter side (`drawRect` in renderer.js);
//!   ellipses are centred on `x, y`.
//! - **Keep**: when the first shape of a frame is `keep`, the frame reuses the
//!   previous frame's resolved shapes (SVGAVideoSpriteEntity.kt, the same in
//!   frameEntity.js).
//! - **Path `d`**: the players tokenize on command letters, treat commas as
//!   spaces and know `M L H V C S Q Z` in both cases; `A` is a no-op in both.
//!   `svgtypes` parses a superset of that (implicit repeats, arcs, exponents)
//!   and accepted every string in a 233-file corpus, which only uses
//!   `M C Z`. A parse error keeps the segments before it, as SVG does.
//! - **Mattes**: a sprite whose `imageKey` ends in `.matte` is never painted.
//!   Consecutive visible sprites with the same `matteKey` are drawn into a
//!   layer, which is then multiplied by the matte sprite's rendered alpha
//!   (`saveLayer` + `PorterDuff.Mode.DST_IN` in SVGACanvasDrawer.kt; a masked
//!   host layer in SVGAPlayer.m). A matte sprite that is invisible on a frame
//!   hides its group; a `matteKey` naming no sprite leaves the group unmasked.
//!   Android and web only enable this when the *first* sprite is a matte; iOS
//!   does not care, and neither does this renderer.

mod draw;
mod encode;
mod image;
mod path;
mod poster;
mod prepare;
#[cfg(test)]
mod tests;

use anyhow::{Result, anyhow, bail};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(crate) use encode::encode_png;

/// Longest output side ever rendered, whatever the caller asks for.
const MAX_SIDE: u32 = 2048;
/// Frame rate players assume when the file has none.
const DEFAULT_FPS: u32 = 20;
const MAX_FPS: u32 = 120;

pub(crate) struct Renderer {
    view_box: (f32, f32),
    fps: u32,
    frame_count: usize,
    poster_frame: usize,
    scene: prepare::Scene,
}

/// One rendered frame as straight (non-premultiplied) RGBA8, row-major.
pub(crate) struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Renderer {
    /// Parse a file and prepare everything a frame needs: decoded images,
    /// parsed paths, resolved `keep` shapes. Fails with a snake_case reason.
    pub(crate) fn new(bytes: &[u8]) -> Result<Self> {
        let animation = svga::Animation::from_bytes_with(bytes, &svga::Limits::default())
            .map_err(|error| anyhow!("{error}"))?;
        let params = animation.movie().params;
        let (width, height) = (params.view_box_width, params.view_box_height);
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            bail!("svga_render_invalid_view_box");
        }
        let frame_count = usize::try_from(params.frames).unwrap_or(0);
        if frame_count == 0 {
            bail!("svga_render_no_frames");
        }
        let scene = prepare::scene(&animation)?;
        Ok(Self {
            view_box: (width, height),
            fps: fps(params.fps),
            frame_count,
            poster_frame: poster::frame(&scene, frame_count),
            scene,
        })
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub(crate) fn fps(&self) -> u32 {
        self.fps
    }

    /// Output size for a longest-side cap: the view box's aspect ratio, never
    /// larger than the view box, never smaller than one pixel.
    pub(crate) fn output_size(&self, max_side: u32) -> (u32, u32) {
        let (width, height) = self.view_box;
        let cap = max_side.clamp(1, MAX_SIDE) as f32;
        let scale = (cap / width.max(height)).min(1.0);
        // Float-to-int `as` saturates, and the cap bounds it anyway.
        let pixels = |side: f32| ((side * scale).round() as u32).clamp(1, MAX_SIDE);
        (pixels(width), pixels(height))
    }

    pub(crate) fn render(&self, frame: usize, max_side: u32) -> Result<Frame> {
        if frame >= self.frame_count {
            bail!("svga_render_frame_out_of_range");
        }
        let (width, height) = self.output_size(max_side);
        let scale = (
            width as f32 / self.view_box.0,
            height as f32 / self.view_box.1,
        );
        // The rasterizer is fed hostile geometry; a panic in it must not take
        // a long-lived server thread down.
        let drawn = catch_unwind(AssertUnwindSafe(|| {
            draw::frame(&self.scene, frame, (width, height), scale)
        }));
        let pixmap = drawn
            .map_err(|_| anyhow!("svga_render_panicked"))?
            .ok_or_else(|| anyhow!("svga_render_canvas_allocation_failed"))?;
        Ok(Frame {
            width,
            height,
            rgba: pixmap.take_demultiplied(),
        })
    }

    /// A representative frame for a thumbnail: the one with the most visible,
    /// drawable, non-matte sprites. Equals are told apart by how much bitmap
    /// they show, then the earliest wins. Decided once, without rasterizing.
    pub(crate) fn poster_frame(&self) -> usize {
        self.poster_frame
    }
}

fn fps(stored: i32) -> u32 {
    match u32::try_from(stored) {
        Ok(0) | Err(_) => DEFAULT_FPS,
        Ok(fps) => fps.min(MAX_FPS),
    }
}
