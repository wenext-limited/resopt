//! Runtime capability report shared by `doctor`, the web UI and the inventory.
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    pub id: &'static str,
    pub available: bool,
    /// Why it is unavailable, or what it is limited to, on this platform.
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub version: &'static str,
    pub platform: &'static str,
    pub features: Vec<Capability>,
    pub tools: Vec<crate::tools::Tool>,
}

const IMAGEIO_NOTE: &str = "Requires Apple ImageIO; available on macOS only.";

/// Formats with a decoder on this platform.
pub(crate) fn decodable(format: &str) -> bool {
    if crate::image_backend_available() {
        matches!(
            format,
            "png" | "jpeg" | "heic" | "heif" | "webp" | "gif" | "tiff" | "bmp" | "avif"
        )
    } else {
        matches!(format, "png" | "webp")
    }
}

/// Whether any optimization backend exists for this resource on this platform.
pub(crate) fn can_optimize(kind: &str, format: &str) -> bool {
    match kind {
        "image" => decodable(format),
        "animation" => format == "svga",
        "archive" => format == "zip",
        _ => false,
    }
}

pub fn capabilities() -> Capabilities {
    let imageio = crate::image_backend_available();
    let imageio_note = if imageio { "" } else { IMAGEIO_NOTE };
    Capabilities {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        features: vec![
            Capability {
                id: "png_lossless",
                available: true,
                note: "",
            },
            Capability {
                id: "webp",
                available: true,
                note: if imageio {
                    ""
                } else {
                    "Inputs limited to PNG/WebP without embedded color profiles or EXIF orientation."
                },
            },
            Capability {
                id: "jpeg",
                available: imageio,
                note: imageio_note,
            },
            Capability {
                id: "heic",
                available: imageio,
                note: imageio_note,
            },
            Capability {
                id: "svga",
                available: true,
                note: "SVGA 2.x: lossless recompression of embedded PNG images.",
            },
            Capability {
                id: "apply_restore",
                available: true,
                note: "",
            },
        ],
        tools: crate::tools::detect(),
    }
}
