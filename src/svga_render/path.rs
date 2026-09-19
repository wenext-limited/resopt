//! Geometry: SVGA path strings, rects and ellipses → `tiny_skia::Path`.
use svgtypes::{SimplePathSegment, SimplifyingPathParser};
use tiny_skia::{Path, PathBuilder, Rect};

/// Control-point distance of the cubic that approximates a quarter circle.
const KAPPA: f32 = 0.552_284_8;

/// A `d` or `clipPath` string. `None` when it yields no drawable geometry. A
/// parse error ends the path and keeps what came before it, as SVG does.
pub(super) fn parse(d: &str) -> Option<Path> {
    let segments = SimplifyingPathParser::from(d).map_while(Result::ok);
    let builder = segments.fold(PathBuilder::new(), |mut builder, segment| {
        // f64 → f32 saturates to infinity, which `finish` rejects.
        match segment {
            SimplePathSegment::MoveTo { x, y } => builder.move_to(x as f32, y as f32),
            SimplePathSegment::LineTo { x, y } => builder.line_to(x as f32, y as f32),
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                builder.quad_to(x1 as f32, y1 as f32, x as f32, y as f32);
            }
            SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => builder.cubic_to(
                x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
            ),
            SimplePathSegment::ClosePath => builder.close(),
        }
        builder
    });
    builder.finish()
}

/// A rectangle whose corner radius is clamped to half its shorter side.
pub(super) fn rect(x: f32, y: f32, width: f32, height: f32, corner_radius: f32) -> Option<Path> {
    let finite = [x, y, width, height].iter().all(|value| value.is_finite());
    if !finite || width < 0.0 || height < 0.0 {
        return None;
    }
    let radius = if corner_radius.is_finite() {
        corner_radius.clamp(0.0, width.min(height) / 2.0)
    } else {
        0.0
    };
    let (right, bottom) = (x + width, y + height);
    let mut builder = PathBuilder::new();
    if radius == 0.0 {
        builder.move_to(x, y);
        builder.line_to(right, y);
        builder.line_to(right, bottom);
        builder.line_to(x, bottom);
        builder.close();
        return builder.finish();
    }
    let control = radius * (1.0 - KAPPA);
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.cubic_to(right - control, y, right, y + control, right, y + radius);
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - control,
        right - control,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(x + radius, bottom);
    builder.cubic_to(x + control, bottom, x, bottom - control, x, bottom - radius);
    builder.line_to(x, y + radius);
    builder.cubic_to(x, y + control, x + control, y, x + radius, y);
    builder.close();
    builder.finish()
}

/// An ellipse centred on `x, y`.
pub(super) fn ellipse(x: f32, y: f32, radius_x: f32, radius_y: f32) -> Option<Path> {
    let (radius_x, radius_y) = (radius_x.abs(), radius_y.abs());
    let bounds = Rect::from_ltrb(x - radius_x, y - radius_y, x + radius_x, y + radius_y)?;
    PathBuilder::from_oval(bounds)
}
