//! Vector shapes: geometry kinds, fill and stroke, transforms, `keep`.
use super::*;
use svga::movie::{Geometry, Rgba, Shape, ShapeStyle};

fn rgba(color: [u8; 4]) -> Rgba {
    let [r, g, b, a] = color.map(|channel| f32::from(channel) / 255.0);
    Rgba { r, g, b, a }
}

fn filled(geometry: Geometry, color: [u8; 4]) -> Shape {
    Shape {
        geometry,
        styles: Some(ShapeStyle {
            fill: Some(rgba(color)),
            ..ShapeStyle::default()
        }),
        transform: None,
    }
}

fn drawing(shapes: Vec<Shape>) -> svga::movie::Frame {
    svga::movie::Frame {
        alpha: 1.0,
        shapes,
        ..svga::movie::Frame::default()
    }
}

fn square(x: f32, y: f32, side: f32) -> Geometry {
    Geometry::Rect {
        x,
        y,
        width: side,
        height: side,
        corner_radius: 0.0,
    }
}

#[test]
fn rects_ellipses_and_paths_fill_and_stroke() {
    let outlined = Shape {
        styles: Some(ShapeStyle {
            fill: Some(rgba(GREEN)),
            stroke: Some(rgba(BLUE)),
            stroke_width: 4.0,
            ..ShapeStyle::default()
        }),
        ..filled(square(10.0, 10.0, 40.0), GREEN)
    };
    let ellipse = Geometry::Ellipse {
        x: 75.0,
        y: 25.0,
        radius_x: 20.0,
        radius_y: 10.0,
    };
    let triangle = Geometry::Path {
        d: "M 10,60 L 40,60 L 40,90 Z".into(),
    };
    let curved = Geometry::Path {
        d: "M60 60C60 60 90 60 90 60c0 0 0 30 0 30L60 90z".into(),
    };
    let shapes = vec![
        outlined,
        filled(ellipse, RED),
        filled(triangle, BLUE),
        filled(curved, RED),
    ];
    let frame = render(vec![sprite("vector", vec![drawing(shapes)])], &[], 0);

    assert_eq!(pixel(&frame, 30, 30), GREEN, "rect fill");
    assert_eq!(
        pixel(&frame, 10, 30),
        BLUE,
        "rect stroke straddles the edge"
    );
    assert_eq!(pixel(&frame, 5, 30), CLEAR, "beyond the stroke");
    assert_eq!(pixel(&frame, 75, 25), RED, "ellipse centre");
    assert_eq!(pixel(&frame, 92, 25), RED, "ellipse is wider than tall");
    assert_eq!(pixel(&frame, 75, 38), CLEAR, "below the ellipse");
    assert_eq!(pixel(&frame, 58, 17), CLEAR, "ellipse corner");
    assert_eq!(pixel(&frame, 35, 70), BLUE, "triangle");
    assert_eq!(pixel(&frame, 15, 80), CLEAR, "under the hypotenuse");
    assert_eq!(pixel(&frame, 75, 75), RED, "relative and compact commands");
}

#[test]
fn a_rounded_rect_loses_its_corners() {
    let rounded = Geometry::Rect {
        x: 10.0,
        y: 10.0,
        width: 80.0,
        height: 80.0,
        // Clamped to half the side: a circle.
        corner_radius: 500.0,
    };
    let frame = render(
        vec![sprite("vector", vec![drawing(vec![filled(rounded, RED)])])],
        &[],
        0,
    );
    assert_eq!(pixel(&frame, 50, 50), RED);
    assert_eq!(pixel(&frame, 50, 12), RED);
    assert_eq!(pixel(&frame, 13, 13), CLEAR);
    assert_eq!(pixel(&frame, 86, 86), CLEAR);
}

#[test]
fn the_shape_transform_applies_before_the_frame_transform() {
    let moved = Shape {
        transform: Some(translate(5.0, 5.0)),
        ..filled(square(0.0, 0.0, 10.0), RED)
    };
    let doubled = svga::movie::Frame {
        transform: Some(Transform {
            a: 2.0,
            d: 2.0,
            ..Transform::IDENTITY
        }),
        ..drawing(vec![moved])
    };
    let frame = render(vec![sprite("vector", vec![doubled])], &[], 0);
    // 2 · (p + 5) covers 10..30; the other order, 2p + 5, would cover 5..25.
    assert_eq!(pixel(&frame, 27, 27), RED);
    assert_eq!(pixel(&frame, 7, 7), CLEAR);
}

#[test]
fn frame_alpha_and_the_clip_path_apply_to_shapes() {
    let half = svga::movie::Frame {
        alpha: 0.5,
        clip_path: "M0 0 L50 0 L50 100 L0 100 Z".into(),
        ..drawing(vec![filled(square(0.0, 0.0, 100.0), BLUE)])
    };
    let frame = render(vec![sprite("vector", vec![half])], &[], 0);
    let [_, _, blue, alpha] = pixel(&frame, 25, 50);
    assert_eq!(blue, 255);
    assert!((127..=128).contains(&alpha), "alpha {alpha}");
    assert_eq!(pixel(&frame, 75, 50), CLEAR);
}

#[test]
fn keep_repeats_the_previous_frames_shapes() {
    let keep = Shape {
        geometry: Geometry::Keep,
        ..Shape::default()
    };
    let frames = vec![
        drawing(vec![filled(square(10.0, 10.0, 20.0), RED)]),
        drawing(vec![keep.clone()]),
        drawing(vec![keep]),
        drawing(Vec::new()),
    ];
    let sprites = vec![sprite("vector", frames)];

    for index in 0..3 {
        let frame = render(sprites.clone(), &[], index);
        assert_eq!(pixel(&frame, 20, 20), RED, "frame {index}");
    }
    assert!(is_blank(&render(sprites, &[], 3)));
}

#[test]
fn a_dashed_stroke_leaves_gaps() {
    let line = Shape {
        geometry: Geometry::Path {
            d: "M0 50 L100 50".into(),
        },
        styles: Some(ShapeStyle {
            stroke: Some(rgba(RED)),
            stroke_width: 6.0,
            line_dash: [10.0, 10.0, 0.0],
            ..ShapeStyle::default()
        }),
        transform: None,
    };
    let frame = render(vec![sprite("vector", vec![drawing(vec![line])])], &[], 0);
    assert_eq!(pixel(&frame, 5, 50), RED);
    assert_eq!(pixel(&frame, 15, 50), CLEAR);
    assert_eq!(pixel(&frame, 25, 50), RED);
}
