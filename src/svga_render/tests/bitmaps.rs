//! Bitmap sprites: placement, alpha, order, layout stretch, clip, mattes.
use super::*;

fn red_square() -> Vec<u8> {
    solid_png(10, 10, RED)
}

#[test]
fn a_translated_bitmap_lands_on_its_rectangle_only() {
    let png = red_square();
    let sprites = vec![sprite("dot", vec![placed(20.0, 30.0, 10.0, 10.0)])];
    let frame = render(sprites, &[("dot", &png)], 0);

    assert_eq!((frame.width, frame.height), (VIEW, VIEW));
    for (x, y) in [(20, 30), (25, 35), (29, 39)] {
        assert_eq!(pixel(&frame, x, y), RED, "inside at {x},{y}");
    }
    for (x, y) in [(0, 0), (19, 35), (30, 35), (25, 29), (25, 40), (99, 99)] {
        assert_eq!(pixel(&frame, x, y), CLEAR, "outside at {x},{y}");
    }
}

#[test]
fn frame_alpha_fades_and_invisible_frames_draw_nothing() {
    let png = red_square();
    let faded = svga::movie::Frame {
        alpha: 0.5,
        ..placed(20.0, 30.0, 10.0, 10.0)
    };
    let hidden = svga::movie::Frame {
        alpha: 0.0,
        ..placed(20.0, 30.0, 10.0, 10.0)
    };
    let over = svga::movie::Frame {
        alpha: 7.0,
        ..placed(20.0, 30.0, 10.0, 10.0)
    };
    let sprites = vec![sprite("dot", vec![faded, hidden, over])];
    let images: [(&str, &[u8]); 1] = [("dot", &png)];

    let [red, green, blue, alpha] = pixel(&render(sprites.clone(), &images, 0), 25, 35);
    assert_eq!((red, green, blue), (255, 0, 0));
    assert!((127..=128).contains(&alpha), "alpha {alpha}");
    assert!(is_blank(&render(sprites.clone(), &images, 1)));
    // Alpha above one is clamped, not wrapped.
    assert_eq!(pixel(&render(sprites, &images, 2), 25, 35), RED);
}

#[test]
fn later_sprites_paint_over_earlier_ones() {
    let (red, blue) = (red_square(), solid_png(10, 10, BLUE));
    let sprites = vec![
        sprite("red", vec![placed(20.0, 20.0, 10.0, 10.0)]),
        sprite("blue", vec![placed(25.0, 25.0, 10.0, 10.0)]),
    ];
    let frame = render(sprites, &[("red", &red), ("blue", &blue)], 0);

    assert_eq!(pixel(&frame, 22, 22), RED);
    assert_eq!(pixel(&frame, 27, 27), BLUE);
    assert_eq!(pixel(&frame, 33, 33), BLUE);
}

#[test]
fn the_layout_box_stretches_the_bitmap() {
    let png = red_square();
    let sprites = vec![sprite("dot", vec![placed(10.0, 10.0, 40.0, 20.0)])];
    let frame = render(sprites, &[("dot", &png)], 0);

    assert_eq!(pixel(&frame, 48, 28), RED);
    assert_eq!(pixel(&frame, 52, 20), CLEAR);
    assert_eq!(pixel(&frame, 30, 32), CLEAR);
}

#[test]
fn a_transform_scales_and_the_canvas_scales_the_view_box() {
    let png = red_square();
    let doubled = svga::movie::Frame {
        transform: Some(Transform {
            a: 2.0,
            d: 2.0,
            ..translate(20.0, 20.0)
        }),
        ..placed(0.0, 0.0, 10.0, 10.0)
    };
    let movie = movie(100.0, 100.0, 1, vec![sprite("dot", vec![doubled])]);
    let renderer = Renderer::new(&file(&movie, &[("dot", &png)])).unwrap();

    let full = renderer.render(0, 100).unwrap();
    assert_eq!(pixel(&full, 38, 38), RED);
    assert_eq!(pixel(&full, 42, 42), CLEAR);
    // Half size: the 20..40 square becomes 10..20.
    let half = renderer.render(0, 50).unwrap();
    assert_eq!((half.width, half.height), (50, 50));
    assert_eq!(pixel(&half, 15, 15), RED);
    assert_eq!(pixel(&half, 22, 15), CLEAR);
    assert_eq!(pixel(&half, 8, 15), CLEAR);
}

#[test]
fn a_clip_path_cuts_the_bitmap_in_sprite_coordinates() {
    let png = red_square();
    let clipped = svga::movie::Frame {
        clip_path: "M0 0 L5 0 L5 10 L0 10 Z".into(),
        ..placed(20.0, 30.0, 10.0, 10.0)
    };
    let nothing_left = svga::movie::Frame {
        clip_path: "no geometry here".into(),
        ..placed(20.0, 30.0, 10.0, 10.0)
    };
    let sprites = vec![sprite("dot", vec![clipped, nothing_left])];
    let images: [(&str, &[u8]); 1] = [("dot", &png)];

    let frame = render(sprites.clone(), &images, 0);
    assert_eq!(pixel(&frame, 22, 35), RED);
    assert_eq!(pixel(&frame, 27, 35), CLEAR);
    assert!(is_blank(&render(sprites, &images, 1)));
}

fn masked(image_key: &str, matte_key: &str, frame: svga::movie::Frame) -> Sprite {
    Sprite {
        matte_key: matte_key.into(),
        ..sprite(image_key, vec![frame])
    }
}

#[test]
fn a_matte_masks_its_group_and_is_not_drawn() {
    let (red, blue, green) = (
        red_square(),
        solid_png(10, 10, BLUE),
        solid_png(4, 4, GREEN),
    );
    let images: [(&str, &[u8]); 3] = [("hole", &blue), ("pic", &red), ("free", &green)];
    let sprites = |matte_alpha: f32| {
        let matte = svga::movie::Frame {
            alpha: matte_alpha,
            ..placed(0.0, 0.0, 50.0, 100.0)
        };
        vec![
            sprite("hole.matte", vec![matte]),
            masked("pic", "hole.matte", placed(0.0, 0.0, 100.0, 50.0)),
            masked("pic", "hole.matte", placed(0.0, 50.0, 100.0, 50.0)),
            sprite("free", vec![placed(80.0, 80.0, 10.0, 10.0)]),
        ]
    };

    let frame = render(sprites(1.0), &images, 0);
    // Inside the matte: the masked sprites, in their own colour.
    assert_eq!(pixel(&frame, 25, 25), RED);
    assert_eq!(pixel(&frame, 25, 75), RED);
    // Outside it they are cut, and the matte's blue bitmap shows nowhere.
    assert_eq!(pixel(&frame, 75, 25), CLEAR);
    assert_eq!(pixel(&frame, 75, 75), CLEAR);
    assert_eq!(pixel(&frame, 85, 85), GREEN);

    // A matte that is invisible on this frame hides its whole group.
    let hidden = render(sprites(0.0), &images, 0);
    assert_eq!(pixel(&hidden, 25, 25), CLEAR);
    assert_eq!(pixel(&hidden, 85, 85), GREEN);
}

#[test]
fn a_matte_key_naming_no_sprite_leaves_the_sprite_unmasked() {
    let red = red_square();
    let sprites = vec![masked("pic", "gone.matte", placed(0.0, 0.0, 100.0, 100.0))];
    let frame = render(sprites, &[("pic", &red)], 0);
    assert_eq!(pixel(&frame, 75, 25), RED);
}
