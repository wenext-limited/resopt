//! Broken and hostile input: refused with a reason or skipped, never a panic.
use super::*;
use svga::movie::{Geometry, Rgba, Shape, ShapeStyle};

fn refusal(movie: &Movie, images: &[(&str, &[u8])]) -> String {
    let error = Renderer::new(&file(movie, images)).err();
    error.expect("refused").to_string()
}

#[test]
fn garbage_is_refused_with_the_codecs_reason() {
    for bytes in [&b""[..], b"not an svga file", b"PK\x03\x04 broken zip"] {
        let error = Renderer::new(bytes).err().expect("refused").to_string();
        assert!(error.starts_with("svga_"), "{error}");
    }
}

#[test]
fn a_movie_without_a_canvas_or_frames_is_refused() {
    for (width, height) in [
        (0.0, 100.0),
        (100.0, -1.0),
        (f32::NAN, 100.0),
        (f32::INFINITY, 1.0),
    ] {
        let reason = refusal(&movie(width, height, 1, Vec::new()), &[]);
        assert_eq!(reason, "svga_render_invalid_view_box");
    }
    for frames in [0, -3] {
        let reason = refusal(&movie(10.0, 10.0, frames, Vec::new()), &[]);
        assert_eq!(reason, "svga_render_no_frames");
    }
}

#[test]
fn oversized_images_are_refused_before_they_are_decoded() {
    // The header alone claims the size; the pixel data is empty.
    let header = |width: u32, height: u32| {
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend([width.to_be_bytes(), height.to_be_bytes()].concat());
        ihdr.extend([8, 6, 0, 0, 0]);
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d".to_vec();
        png.extend(&ihdr);
        png.extend(crc32(&ihdr).to_be_bytes());
        png.extend(0u32.to_be_bytes());
        png.extend(b"IDAT");
        png.extend(crc32(b"IDAT").to_be_bytes());
        png
    };
    let drawing = |keys: &[&str]| {
        let sprites = keys
            .iter()
            .map(|key| sprite(key, vec![placed(0.0, 0.0, 1.0, 1.0)]));
        movie(10.0, 10.0, 1, sprites.collect())
    };

    let wide = header(100_000, 1);
    let reason = refusal(&drawing(&["a"]), &[("a", &wide)]);
    assert_eq!(reason, "svga_render_image_too_large");

    let big = header(4096, 4096);
    let keys = ["a", "b", "c", "d", "e"];
    let images = keys.map(|key| (key, big.as_slice()));
    // Unreferenced images cost nothing, four fit, the fifth exceeds 64M.
    assert!(Renderer::new(&file(&drawing(&[]), &images)).is_ok());
    assert!(Renderer::new(&file(&drawing(&keys[..4]), &images)).is_ok());
    let reason = refusal(&drawing(&keys), &images);
    assert_eq!(reason, "svga_render_image_pixels_exceed_limit");
}

fn crc32(bytes: &[u8]) -> u32 {
    let step = |crc: u32, byte: &u8| {
        (0..8).fold(crc ^ u32::from(*byte), |crc, _| {
            (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1))
        })
    };
    !bytes.iter().fold(u32::MAX, step)
}

#[test]
fn sprites_that_cannot_be_drawn_are_skipped() {
    let red = solid_png(10, 10, RED);
    let truncated = &red[..red.len() / 2];
    let images: [(&str, &[u8]); 4] = [
        ("ok", &red),
        ("name", b"img_12"),
        ("audio", b"ID3\x03 not an image"),
        ("cut", truncated),
    ];
    let non_finite = svga::movie::Frame {
        transform: Some(Transform {
            a: f32::NAN,
            ..translate(0.0, 0.0)
        }),
        ..placed(0.0, 0.0, 100.0, 100.0)
    };
    let nan_alpha = svga::movie::Frame {
        alpha: f32::NAN,
        ..placed(0.0, 0.0, 100.0, 100.0)
    };
    let flat = placed(0.0, 0.0, 0.0, f32::INFINITY);
    let far = placed(f32::MAX, -f32::MAX, 10.0, 10.0);
    let sprites = vec![
        sprite("missing", vec![placed(0.0, 0.0, 100.0, 100.0)]),
        sprite("name", vec![placed(0.0, 0.0, 100.0, 100.0)]),
        sprite("audio", vec![placed(0.0, 0.0, 100.0, 100.0)]),
        sprite("cut", vec![placed(0.0, 0.0, 100.0, 100.0)]),
        sprite("", vec![placed(0.0, 0.0, 100.0, 100.0)]),
        sprite("ok", vec![non_finite]),
        sprite("ok", vec![nan_alpha]),
        sprite("ok", vec![flat]),
        sprite("ok", vec![far]),
        sprite("ok", Vec::new()),
        sprite("ok", vec![placed(90.0, 90.0, 10.0, 10.0)]),
    ];
    let frame = render(sprites, &images, 0);
    assert_eq!(pixel(&frame, 95, 95), RED);
    assert_eq!(pixel(&frame, 50, 50), CLEAR);
}

#[test]
fn malformed_shapes_are_skipped() {
    let style = |stroke_width: f32, line_dash: [f32; 3]| ShapeStyle {
        fill: Some(Rgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        }),
        stroke: Some(Rgba {
            // Out of range is clamped; not a number paints nothing.
            r: if stroke_width.is_finite() {
                0.5
            } else {
                f32::NAN
            },
            g: 2.0,
            b: -1.0,
            a: 1.0,
        }),
        stroke_width,
        miter_limit: f32::NAN,
        line_dash,
        ..ShapeStyle::default()
    };
    let path = |d: &str| Shape {
        geometry: Geometry::Path { d: d.into() },
        styles: Some(style(f32::INFINITY, [f32::NAN, f32::INFINITY, f32::NAN])),
        transform: None,
    };
    let rect = |width: f32, corner_radius: f32, skew: f32| Shape {
        geometry: Geometry::Rect {
            x: 0.0,
            y: 0.0,
            width,
            height: 10.0,
            corner_radius,
        },
        styles: Some(style(1e30, [1e30, 0.0, 1e30])),
        transform: Some(Transform {
            c: skew,
            ..translate(0.0, 0.0)
        }),
    };
    let shapes = vec![
        path(""),
        path("banana"),
        path("L 10 10"),
        path("M 1e999 0 L 5 5 Z"),
        path("M 0 0 L 1e38 1e38 L -1e38 1e38 Z"),
        path("M 0 0 A 0 0 0 0 0 5 5 Z"),
        rect(-5.0, 1.0, 0.0),
        rect(f32::NAN, f32::NAN, 0.0),
        rect(10.0, 1.0, f32::INFINITY),
        // Drawable, with an absurd stroke and dash.
        rect(10.0, f32::NAN, 1e30),
        Shape {
            geometry: Geometry::Ellipse {
                x: f32::NAN,
                y: 0.0,
                radius_x: -1.0,
                radius_y: f32::INFINITY,
            },
            styles: Some(style(1.0, [0.0; 3])),
            transform: None,
        },
        Shape {
            geometry: Geometry::Keep,
            ..Shape::default()
        },
        // Valid up to the error: the square before it still draws.
        path("M 60 60 L 90 60 L 90 90 L 60 90 Z L banana"),
    ];
    let drawing = svga::movie::Frame {
        alpha: 1.0,
        shapes,
        ..svga::movie::Frame::default()
    };
    let frame = render(vec![sprite("vector", vec![drawing])], &[], 0);
    assert_eq!(pixel(&frame, 75, 75), BLUE);
}
