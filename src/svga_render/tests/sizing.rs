//! Output size, frame bookkeeping, the poster choice and PNG output.
use super::*;
use std::io::Cursor;

fn renderer(width: f32, height: f32, frames: i32, sprites: Vec<Sprite>) -> Renderer {
    let png = solid_png(2, 2, RED);
    Renderer::new(&file(
        &movie(width, height, frames, sprites),
        &[("dot", &png)],
    ))
    .unwrap()
}

fn shown(alpha: f32) -> svga::movie::Frame {
    svga::movie::Frame {
        alpha,
        ..placed(0.0, 0.0, 10.0, 10.0)
    }
}

#[test]
fn the_output_keeps_the_aspect_ratio_within_both_caps() {
    let wide = renderer(200.0, 100.0, 1, Vec::new());
    assert_eq!(wide.output_size(50), (50, 25));
    assert_eq!(wide.output_size(200), (200, 100));
    // Never upscaled past the view box, never below one pixel.
    assert_eq!(wide.output_size(1000), (200, 100));
    assert_eq!(wide.output_size(u32::MAX), (200, 100));
    assert_eq!(wide.output_size(0), (1, 1));

    let huge = renderer(8192.0, 100.0, 1, Vec::new());
    assert_eq!(huge.output_size(u32::MAX), (2048, 25));
    let sliver = renderer(0.25, 1000.0, 1, Vec::new());
    assert_eq!(sliver.output_size(100), (1, 100));

    let frame = wide.render(0, 50).unwrap();
    assert_eq!((frame.width, frame.height), (50, 25));
    assert_eq!(frame.rgba.len(), 50 * 25 * 4);
}

#[test]
fn frames_and_fps_come_from_the_params() {
    let plain = renderer(10.0, 10.0, 3, Vec::new());
    assert_eq!((plain.frame_count(), plain.fps()), (3, 30));
    assert!(plain.render(2, 10).is_ok());
    let error = plain.render(3, 10).err().unwrap();
    assert_eq!(error.to_string(), "svga_render_frame_out_of_range");

    let fps = |stored: i32| {
        let mut movie = movie(10.0, 10.0, 1, Vec::new());
        movie.params.fps = stored;
        Renderer::new(&file(&movie, &[])).unwrap().fps()
    };
    assert_eq!((fps(0), fps(-5), fps(24), fps(100_000)), (20, 20, 24, 120));
}

#[test]
fn the_poster_is_the_earliest_frame_with_the_most_drawn_sprites() {
    let sprites = vec![
        sprite("dot", vec![shown(0.0), shown(1.0), shown(1.0), shown(1.0)]),
        sprite("dot", vec![shown(0.0), shown(0.0), shown(1.0), shown(1.0)]),
        // Mattes and sprites with nothing to draw do not count.
        sprite(
            "dot.matte",
            vec![shown(1.0), shown(1.0), shown(0.0), shown(0.0)],
        ),
        sprite(
            "nothing",
            vec![shown(1.0), shown(1.0), shown(0.0), shown(0.0)],
        ),
        // Shorter than the movie: simply absent from the later frames.
        sprite("dot", vec![shown(0.0)]),
    ];
    assert_eq!(renderer(10.0, 10.0, 4, sprites).poster_frame(), 2);
    assert_eq!(renderer(10.0, 10.0, 4, Vec::new()).poster_frame(), 0);
}

#[test]
fn equally_busy_frames_are_told_apart_by_the_bitmap_they_show() {
    // A bitmap sequence: one sprite per frame, the first one empty.
    let (empty, faint, solid) = (
        solid_png(2, 2, CLEAR),
        solid_png(2, 2, [255, 0, 0, 40]),
        solid_png(2, 2, RED),
    );
    let only_on = |index: usize| {
        let frames = (0..4).map(|frame| shown(if frame == index { 1.0 } else { 0.0 }));
        frames.collect::<Vec<_>>()
    };
    let sprites = vec![
        sprite("empty", only_on(0)),
        sprite("faint", only_on(1)),
        sprite("solid", only_on(2)),
        sprite("solid", only_on(3)),
    ];
    let images: [(&str, &[u8]); 3] = [("empty", &empty), ("faint", &faint), ("solid", &solid)];
    let bytes = file(&movie(10.0, 10.0, 4, sprites), &images);
    assert_eq!(Renderer::new(&bytes).unwrap().poster_frame(), 2);
}

#[test]
fn a_frame_round_trips_through_png() {
    let sprites = vec![sprite("dot", vec![placed(2.0, 2.0, 4.0, 4.0)])];
    let frame = renderer(8.0, 6.0, 1, sprites).render(0, 8).unwrap();
    let bytes = encode_png_checked(&frame);

    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (8, 6));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(&pixels[..info.buffer_size()], frame.rgba.as_slice());
    assert_eq!(pixel(&frame, 3, 3), RED);
}

fn encode_png_checked(frame: &Frame) -> Vec<u8> {
    let lying = Frame {
        width: frame.width + 1,
        height: frame.height,
        rgba: frame.rgba.clone(),
    };
    assert!(crate::svga_render::encode_png(&lying).is_err());
    crate::svga_render::encode_png(frame).unwrap()
}

#[test]
fn one_renderer_serves_several_threads() {
    fn shareable<T: Send + Sync>() {}
    shareable::<Renderer>();

    let sprites = vec![sprite("dot", vec![placed(0.0, 0.0, 10.0, 10.0); 4])];
    let renderer = renderer(10.0, 10.0, 4, sprites);
    let renderer = &renderer;
    let corners: Vec<[u8; 4]> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|frame| scope.spawn(move || pixel(&renderer.render(frame, 10).unwrap(), 5, 5)))
            .collect();
        let finished = workers.into_iter().map(|worker| worker.join().unwrap());
        finished.collect()
    });
    assert_eq!(corners, [RED; 4]);
}

#[test]
fn canvas_size_reports_the_real_view_box_even_above_the_render_cap() {
    let red = solid_png(2, 2, [255, 0, 0, 255]);
    let bytes = file(
        &movie(
            3000.0,
            1500.4,
            1,
            vec![sprite("a", vec![placed(0.0, 0.0, 2.0, 2.0)])],
        ),
        &[("a", &red)],
    );
    let renderer = Renderer::new(&bytes).unwrap();
    assert_eq!(renderer.canvas_size(), (3000, 1500));
    // Rendering stays capped; only the reported canvas is uncapped.
    assert_eq!(renderer.output_size(u32::MAX), (2048, 1024));
}
