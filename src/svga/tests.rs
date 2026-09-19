mod analysis;

use super::*;
use svga::{Document, Limits, wire};

const SIDE: u32 = 64;

fn field(number: u32, payload: &[u8]) -> Vec<u8> {
    wire::length_delimited(number, payload)
}

fn float(number: u8, value: f32) -> Vec<u8> {
    [vec![(number << 3) | 5], value.to_le_bytes().to_vec()].concat()
}

fn varint_field(number: u8, value: u8) -> Vec<u8> {
    vec![number << 3, value]
}

/// A smooth gradient stored without compression, so there is room to shrink.
fn png_image(seed: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, SIDE, SIDE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::NoCompression);
    let data: Vec<u8> = (0..SIDE * SIDE)
        .flat_map(|i| [(i % SIDE) as u8, (i / SIDE) as u8, seed, 255])
        .collect();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    bytes
}

fn animated_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_animated(2, 0).unwrap();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[0; 16]).unwrap();
    writer.write_image_data(&[255; 16]).unwrap();
    writer.finish().unwrap();
    bytes
}

fn image_entry(key: &str, value: &[u8]) -> Vec<u8> {
    field(3, &[field(1, key.as_bytes()), field(2, value)].concat())
}

/// A sprite whose frame carries field 15 and which itself carries field 99,
/// neither of which exists in the SVGA schema.
fn sprite(alpha: f32) -> Vec<u8> {
    let layout = [float(1, 0.0), float(2, 0.0), float(3, 64.0), float(4, 64.0)].concat();
    let frame = [float(1, alpha), field(2, &layout), varint_field(15, 42)].concat();
    let body = [
        field(1, b"img_0"),
        field(2, &frame),
        field(2, &frame),
        field(99, b"vendor extension"),
    ]
    .concat();
    field(4, &body)
}

fn header() -> Vec<u8> {
    let params = [
        float(1, 64.0),
        float(2, 64.0),
        varint_field(3, 20),
        varint_field(4, 2),
    ]
    .concat();
    [field(1, b"2.1.0"), field(2, &params)].concat()
}

fn movie_with(images: &[(&str, Vec<u8>)], alpha: f32, tail: &[u8]) -> Vec<u8> {
    let entries: Vec<u8> = images
        .iter()
        .flat_map(|(key, value)| image_entry(key, value))
        .collect();
    [header(), entries, sprite(alpha), tail.to_vec()].concat()
}

fn images() -> Vec<(&'static str, Vec<u8>)> {
    vec![("img_0", png_image(1)), ("img_1", png_image(2))]
}

const STORED_BLOCK: usize = 65_535;

/// A zlib stream of stored blocks. Written by hand so malformed payloads,
/// which the `svga` crate refuses to encode, can be wrapped too.
fn pack(proto: &[u8]) -> Vec<u8> {
    let blocks: Vec<&[u8]> = if proto.is_empty() {
        vec![proto]
    } else {
        proto.chunks(STORED_BLOCK).collect()
    };
    let last = blocks.len() - 1;
    let body = blocks.iter().enumerate().flat_map(|(index, block)| {
        let length = block.len() as u16;
        [
            &[u8::from(index == last)][..],
            &length.to_le_bytes(),
            &(!length).to_le_bytes(),
            block,
        ]
        .concat()
    });
    let (a, b) = proto.iter().fold((1u32, 0u32), |(a, b), byte| {
        let a = (a + u32::from(*byte)) % 65_521;
        (a, (b + a) % 65_521)
    });
    let adler = (b << 16) | a;
    [0x78, 0x01]
        .into_iter()
        .chain(body)
        .chain(adler.to_be_bytes())
        .collect()
}

fn unpack(bytes: &[u8]) -> Vec<u8> {
    Document::from_bytes(bytes).unwrap().to_proto()
}

fn sample() -> Vec<u8> {
    pack(&movie_with(&images(), 1.0, &[]))
}

fn reason(result: Result<impl Sized>) -> String {
    result.err().expect("expected an error").to_string()
}

fn refusal(bytes: &[u8]) -> Refusal {
    *optimize(bytes, &Policy::default())
        .unwrap_err()
        .downcast_ref::<Refusal>()
        .expect("typed refusal")
}

#[test]
fn optimize_shrinks_verifies_and_keeps_unknown_nested_fields_verbatim() {
    let original = sample();
    let optimized = optimize(&original, &Policy::default()).unwrap();
    assert!(optimized.len() < original.len());
    verify(&original, &optimized).unwrap();
    let proto = unpack(&optimized);
    let sprite = sprite(1.0);
    assert!(proto.windows(sprite.len()).any(|window| window == sprite));
    assert!(proto.starts_with(&header()));
    // Both embedded PNGs were replaced by smaller, pixel-identical ones.
    let values = |bytes: &[u8]| -> Vec<usize> {
        let document = Document::from_bytes(bytes).unwrap();
        document.images().map(|image| image.value().len()).collect()
    };
    let (before, after) = (values(&original), values(&optimized));
    assert_eq!(before.len(), 2);
    assert!(before.iter().zip(&after).all(|(old, new)| new < old));
}

#[test]
fn optimize_returns_the_original_when_nothing_is_gained() {
    let once = optimize(&sample(), &Policy::default()).unwrap();
    let twice = optimize(&once, &Policy::default()).unwrap();
    assert_eq!(once, twice);
    verify(&once, &twice).unwrap();
}

#[test]
fn file_name_values_are_kept_and_unoptimizable_pngs_do_not_fail_the_file() {
    let broken = [b"\x89PNG\r\n\x1a\n".as_slice(), b"not really a png"].concat();
    let images = [
        ("img_0", png_image(1)),
        ("name", b"chest.png".to_vec()),
        ("bad", broken),
    ];
    let original = pack(&movie_with(&images, 1.0, &[]));
    let optimized = optimize(&original, &Policy::default()).unwrap();
    assert!(optimized.len() < original.len());
    verify(&original, &optimized).unwrap();
}

#[test]
fn verify_rejects_a_changed_non_image_field() {
    let original = sample();
    let changed = pack(&movie_with(&images(), 0.5, &[]));
    assert_eq!(
        reason(verify(&original, &changed)),
        "svga_verify_field_changed"
    );
}

#[test]
fn verify_rejects_a_changed_pixel() {
    let original = sample();
    let changed = pack(&movie_with(
        &[("img_0", png_image(1)), ("img_1", png_image(3))],
        1.0,
        &[],
    ));
    assert_eq!(
        reason(verify(&original, &changed)),
        "svga_verify_image_pixels_changed"
    );
}

#[test]
fn verify_rejects_renamed_reordered_and_missing_images() {
    let original = sample();
    let renamed = pack(&movie_with(
        &[("img_0", png_image(1)), ("img_2", png_image(2))],
        1.0,
        &[],
    ));
    let reordered = pack(&movie_with(
        &[("img_1", png_image(2)), ("img_0", png_image(1))],
        1.0,
        &[],
    ));
    for changed in [renamed, reordered] {
        assert_eq!(
            reason(verify(&original, &changed)),
            "svga_verify_image_key_changed"
        );
    }
    let missing = pack(&movie_with(&[("img_0", png_image(1))], 1.0, &[]));
    assert_eq!(
        reason(verify(&original, &missing)),
        "svga_verify_field_count_changed"
    );
    let file_name = pack(&movie_with(
        &[("img_0", png_image(1)), ("img_1", b"img_1.png".to_vec())],
        1.0,
        &[],
    ));
    assert_eq!(
        reason(verify(&original, &file_name)),
        "svga_verify_image_value_changed"
    );
}

#[test]
fn verify_rejects_trailing_bytes() {
    let original = sample();
    let optimized = optimize(&original, &Policy::default()).unwrap();
    let padded = [optimized, vec![0]].concat();
    assert_eq!(reason(verify(&original, &padded)), "svga_trailing_bytes");
    assert_eq!(refusal(&padded), Refusal::Malformed("svga_trailing_bytes"));
}

#[test]
fn unsupported_animations_are_refused_with_a_reason() {
    let audio = field(5, &field(1, b"audio_0"));
    let cases = [
        (b"PK\x03\x04zip".to_vec(), "svga_1x_zip_not_supported"),
        (
            pack(&movie_with(&images(), 1.0, &audio)),
            "svga_contains_audio",
        ),
        (
            pack(&movie_with(&[("img_0", animated_png())], 1.0, &[])),
            "svga_animated_png_not_supported",
        ),
        (
            pack(&movie_with(&images(), 1.0, &field(6, b"future"))),
            "svga_unknown_field",
        ),
        (
            pack(&movie_with(
                &[("img_0", vec![0xff, 0xfb, 0x90, 0x00])],
                1.0,
                &[],
            )),
            "svga_non_png_image_not_supported",
        ),
        (
            pack(&movie_with(
                &[("img_0", png_image(1)), ("img_0", png_image(2))],
                1.0,
                &[],
            )),
            "svga_duplicate_image_key",
        ),
        (
            pack(&[field(1, b"1.5.0"), sprite(1.0)].concat()),
            "svga_unsupported_version",
        ),
        (pack(&sprite(1.0)), "svga_unsupported_version"),
    ];
    for (bytes, expected) in cases {
        assert_eq!(refusal(&bytes), Refusal::Unsupported(expected));
    }
    // An unknown field inside a map entry is refused as well.
    let entry = [field(1, b"img_0"), field(2, &png_image(1)), field(3, b"x")].concat();
    let bytes = pack(&[header(), field(3, &entry)].concat());
    assert_eq!(refusal(&bytes), Refusal::Unsupported("svga_unknown_field"));
}

#[test]
fn malformed_input_is_rejected_without_panicking() {
    let truncated_varint = [header(), vec![0x22, 0x80]].concat();
    let truncated_field = [header(), vec![0x22, 0x7f, 1, 2, 3]].concat();
    let overlong_varint = [header(), vec![0xff; 11]].concat();
    let group = [header(), vec![0x23]].concat();
    let cases = [
        (pack(&truncated_varint), "svga_truncated_varint"),
        (pack(&truncated_field), "svga_truncated_field"),
        (pack(&overlong_varint), "svga_varint_overflow"),
        (pack(&group), "svga_unsupported_wire_type"),
        (
            pack(&[header(), vec![0x00, 0x00]].concat()),
            "svga_invalid_field_number",
        ),
        (
            pack(&[header(), vec![0x20, 0x01]].concat()),
            "svga_unexpected_wire_type",
        ),
        (b"definitely not an svga file".to_vec(), "svga_not_zlib"),
        (Vec::new(), "svga_not_zlib"),
        (vec![0x78], "svga_not_zlib"),
        (vec![0x78, 0x9c, 1, 2, 3, 4], "svga_corrupt_zlib_stream"),
    ];
    for (bytes, expected) in cases {
        assert_eq!(refusal(&bytes), Refusal::Malformed(expected), "{expected}");
    }
    let whole = sample();
    for length in 0..whole.len() {
        assert!(optimize(&whole[..length], &Policy::default()).is_err());
    }
}

#[test]
fn zlib_bombs_stop_at_the_inflated_size_cap() {
    // Sprites are opaque, so a megabyte of zeros is a valid, compressible movie.
    let proto = [header(), field(4, &vec![0; 1024 * 1024])].concat();
    let bomb = Document::from_proto(proto.clone())
        .unwrap()
        .to_bytes(Compression::Fast)
        .unwrap();
    assert!(bomb.len() < 8 * 1024);
    let capped = |limit| rules::open_with(&bomb, &Limits::default().with_max_inflated_bytes(limit));
    for limit in [64 * 1024, proto.len() - 1] {
        assert_eq!(
            capped(limit).err(),
            Some(Refusal::Unsupported("svga_inflated_size_exceeds_limit"))
        );
    }
    assert_eq!(capped(proto.len()).unwrap().to_proto(), proto);
    let tiny = Limits::default().with_max_input_bytes(bomb.len() - 1);
    assert_eq!(
        rules::open_with(&bomb, &tiny).err(),
        Some(Refusal::Unsupported("svga_input_exceeds_limit"))
    );
}

/// A key that is not valid UTF-8 cannot be addressed by name; entries are
/// edited by position, so such an image is optimized like any other and the
/// key bytes survive untouched.
#[test]
fn an_image_under_a_non_utf8_key_is_optimized_and_keeps_its_key_bytes() {
    let odd_key: &[u8] = &[0xff, 0xfe, b'k'];
    let entry = |key: &[u8], value: &[u8]| field(3, &[field(1, key), field(2, value)].concat());
    let proto = [
        header(),
        entry(odd_key, &png_image(1)),
        entry(b"img_1", &png_image(2)),
        sprite(1.0),
    ]
    .concat();
    let original = pack(&proto);
    let optimized = optimize(&original, &Policy::default()).unwrap();
    verify(&original, &optimized).unwrap();
    let read = |bytes: &[u8]| -> Vec<(Vec<u8>, usize)> {
        Document::from_bytes(bytes)
            .unwrap()
            .images()
            .map(|image| (image.key_bytes().to_vec(), image.value().len()))
            .collect()
    };
    let (before, after) = (read(&original), read(&optimized));
    assert_eq!(after[0].0, odd_key);
    assert_eq!(after[1].0, b"img_1");
    assert!(before.iter().zip(&after).all(|(old, new)| new.1 < old.1));
}
