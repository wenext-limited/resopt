//! The top level of a `MovieEntity`. Only the `images` map is understood; the
//! version, params and sprites are opaque byte ranges that are never rewritten.
use super::{
    Refusal,
    wire::{self, Field, LENGTH_DELIMITED},
};
use std::collections::HashSet;

const VERSION: u32 = 1;
const PARAMS: u32 = 2;
const IMAGES: u32 = 3;
const SPRITES: u32 = 4;
const AUDIOS: u32 = 5;
const ENTRY_KEY: u32 = 1;
const ENTRY_VALUE: u32 = 2;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAX_FILE_NAME_BYTES: usize = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueKind {
    Png,
    /// A reference to a file shipped next to the animation.
    FileName,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Image<'a> {
    pub key: &'a [u8],
    pub value: &'a [u8],
    pub kind: ValueKind,
    /// Key and value fields in their stored order.
    entry: [Field<'a>; 2],
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Part<'a> {
    Verbatim(Field<'a>),
    Image(Field<'a>, Image<'a>),
}

impl<'a> Part<'a> {
    pub fn field(&self) -> &Field<'a> {
        match self {
            Part::Verbatim(field) | Part::Image(field, _) => field,
        }
    }
}

/// Parse and validate. Anything outside the known-safe subset is refused.
pub(super) fn parse(proto: &[u8]) -> Result<Vec<Part<'_>>, Refusal> {
    let fields = wire::walk(proto)?;
    let mut keys = HashSet::new();
    let mut singular = HashSet::new();
    let mut parts = Vec::with_capacity(fields.len());
    for field in fields {
        match field.number {
            AUDIOS => return Err(Refusal::Unsupported("svga_contains_audio")),
            VERSION | PARAMS | IMAGES | SPRITES => {}
            _ => return Err(Refusal::Unsupported("svga_unknown_field")),
        }
        if field.wire_type != LENGTH_DELIMITED {
            return Err(Refusal::Malformed("svga_unexpected_wire_type"));
        }
        if matches!(field.number, VERSION | PARAMS) && !singular.insert(field.number) {
            return Err(Refusal::Unsupported("svga_duplicate_field"));
        }
        if field.number == VERSION && !field.payload.starts_with(b"2.") {
            return Err(Refusal::Unsupported("svga_unsupported_version"));
        }
        if field.number != IMAGES {
            parts.push(Part::Verbatim(field));
            continue;
        }
        let image = image(&field)?;
        if !keys.insert(image.key) {
            return Err(Refusal::Unsupported("svga_duplicate_image_key"));
        }
        parts.push(Part::Image(field, image));
    }
    if !singular.contains(&VERSION) {
        return Err(Refusal::Unsupported("svga_unsupported_version"));
    }
    Ok(parts)
}

fn image<'a>(field: &Field<'a>) -> Result<Image<'a>, Refusal> {
    let entry: [Field<'a>; 2] = wire::walk(field.payload)?
        .try_into()
        .map_err(|_| Refusal::Unsupported("svga_unknown_field"))?;
    let find = |number| {
        entry
            .iter()
            .find(|part| part.number == number && part.wire_type == LENGTH_DELIMITED)
            .map(|part| part.payload)
            .ok_or(Refusal::Unsupported("svga_unknown_field"))
    };
    let (key, value) = (find(ENTRY_KEY)?, find(ENTRY_VALUE)?);
    Ok(Image {
        key,
        value,
        kind: value_kind(value)?,
        entry,
    })
}

fn value_kind(value: &[u8]) -> Result<ValueKind, Refusal> {
    if value.starts_with(PNG_SIGNATURE) {
        if is_animated(value) {
            return Err(Refusal::Unsupported("svga_animated_png_not_supported"));
        }
        return Ok(ValueKind::Png);
    }
    let is_file_name = value.len() <= MAX_FILE_NAME_BYTES
        && std::str::from_utf8(value).is_ok_and(|name| !name.chars().any(char::is_control));
    if is_file_name {
        Ok(ValueKind::FileName)
    } else {
        Err(Refusal::Unsupported("svga_non_png_image_not_supported"))
    }
}

/// `acTL` must precede `IDAT`, so the walk stops at the first image data.
fn is_animated(png: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    while let Some(header) = offset.checked_add(8).and_then(|end| png.get(offset..end)) {
        let kind = &header[4..];
        if kind == b"acTL" {
            return true;
        }
        if kind == b"IDAT" || kind == b"IEND" {
            return false;
        }
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        // Length, type, data and CRC.
        let Some(next) = length
            .checked_add(12)
            .and_then(|size| offset.checked_add(size))
        else {
            return false;
        };
        offset = next;
    }
    false
}

/// The `images` field for `image` with its value replaced; the key field and
/// the field order are kept as stored.
pub(super) fn with_value(image: &Image<'_>, value: &[u8]) -> Vec<u8> {
    let entry: Vec<u8> = image
        .entry
        .iter()
        .flat_map(|part| {
            if part.number == ENTRY_VALUE {
                wire::length_delimited(ENTRY_VALUE, value)
            } else {
                part.raw.to_vec()
            }
        })
        .collect();
    wire::length_delimited(IMAGES, &entry)
}

/// Field numbers of the map entry in stored order.
pub(super) fn entry_order(image: &Image<'_>) -> [u32; 2] {
    [image.entry[0].number, image.entry[1].number]
}
