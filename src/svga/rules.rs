//! The subset of SVGA 2.x that is rewritten. The `svga` crate reads far more
//! (audio, unknown fields, repeated keys, any version); everything outside the
//! known-safe subset is refused here instead.
use crate::optimizer;
use std::{collections::HashSet, fmt};
use svga::{Document, ErrorKind, ImageEntry, Limits, ValueKind};

const VERSION: u32 = 1;
const PARAMS: u32 = 2;
pub(super) const IMAGES: u32 = 3;
const SPRITES: u32 = 4;
const AUDIOS: u32 = 5;

const MAX_INFLATED: usize = 256 * 1024 * 1024;
const MAX_FILE_NAME_BYTES: usize = 255;

/// Why a file is left alone. The reason is a stable machine-readable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A valid animation outside the subset that can be rewritten safely.
    Unsupported(&'static str),
    Malformed(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Refusal::Unsupported(reason) | Refusal::Malformed(reason)) = self;
        formatter.write_str(reason)
    }
}

impl std::error::Error for Refusal {}

impl From<svga::Error> for Refusal {
    fn from(error: svga::Error) -> Self {
        match (error.kind(), error.code()) {
            (_, "svga_zip_container") => Refusal::Unsupported("svga_1x_zip_not_supported"),
            // A file over a size bound may be perfectly valid.
            (ErrorKind::Unsupported | ErrorKind::LimitExceeded, code) => Refusal::Unsupported(code),
            (_, code) => Refusal::Malformed(code),
        }
    }
}

pub(super) fn limits() -> Limits {
    Limits::default()
        .with_max_input_bytes(optimizer::MAX_INPUT)
        .with_max_inflated_bytes(MAX_INFLATED)
}

/// Read a file and refuse it unless every field is one that can be kept or
/// rewritten safely.
pub(super) fn open(bytes: &[u8]) -> Result<Document, Refusal> {
    open_with(bytes, &limits())
}

pub(super) fn open_with(bytes: &[u8], limits: &Limits) -> Result<Document, Refusal> {
    let document = Document::from_bytes_with(bytes, limits)?;
    check(&document)?;
    Ok(document)
}

fn check(document: &Document) -> Result<(), Refusal> {
    let mut images = document.images();
    let mut keys = HashSet::new();
    let mut singular = HashSet::new();
    for field in document.fields() {
        match field.number {
            AUDIOS => return Err(Refusal::Unsupported("svga_contains_audio")),
            VERSION | PARAMS | IMAGES | SPRITES => {}
            _ => return Err(Refusal::Unsupported("svga_unknown_field")),
        }
        if matches!(field.number, VERSION | PARAMS) && !singular.insert(field.number) {
            return Err(Refusal::Unsupported("svga_duplicate_field"));
        }
        if field.number == VERSION && !field.payload.starts_with(b"2.") {
            return Err(Refusal::Unsupported("svga_unsupported_version"));
        }
        if field.number != IMAGES {
            continue;
        }
        // Every `images` field has an entry; they are listed in stored order.
        let Some(image) = images.next() else {
            return Err(Refusal::Malformed("svga_truncated_field"));
        };
        check_image(&image)?;
        if !keys.insert(image.key_bytes()) {
            return Err(Refusal::Unsupported("svga_duplicate_image_key"));
        }
    }
    if !singular.contains(&VERSION) {
        return Err(Refusal::Unsupported("svga_unsupported_version"));
    }
    Ok(())
}

fn check_image(image: &ImageEntry<'_>) -> Result<(), Refusal> {
    if !image.is_canonical() {
        return Err(Refusal::Unsupported("svga_unknown_field"));
    }
    let value = image.value();
    if image.kind() == ValueKind::Png {
        if svga::is_animated_png(value) {
            return Err(Refusal::Unsupported("svga_animated_png_not_supported"));
        }
        return Ok(());
    }
    // A reference to a file shipped next to the animation; kept verbatim.
    let is_file_name = value.len() <= MAX_FILE_NAME_BYTES
        && std::str::from_utf8(value).is_ok_and(|name| !name.chars().any(char::is_control));
    if is_file_name {
        Ok(())
    } else {
        Err(Refusal::Unsupported("svga_non_png_image_not_supported"))
    }
}
