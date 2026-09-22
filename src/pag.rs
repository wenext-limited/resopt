//! Content-based PAG recognition and an offline, pinned browser renderer.
//! Header inspection is not a claim of decoder compatibility or visual fidelity.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) const RUNTIME_FRAME: &str = "previews/0-pag-player.html";
pub(crate) const RUNTIME_WASM: &str = "previews/0-libpag-4.3.51.wasm";
pub(crate) const RUNTIME_LICENSE: &str = "previews/0-libpag-license.txt";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagInfo {
    pub container_version: u8,
    pub runtime_version: String,
}

pub(crate) fn inspect(bytes: &[u8]) -> Result<PagInfo> {
    ensure!(
        bytes.len() >= 11 && bytes.starts_with(b"PAG"),
        "pag_invalid_header"
    );
    ensure!(bytes.len() <= 16 * 1024 * 1024, "pag_preview_input_limit");
    ensure!(
        bytes[3] == 1 && bytes[8] == b'U',
        "pag_unsupported_version_or_compression"
    );
    // PAG writes the body length, excluding its nine-byte header (Codec.cpp).
    let body = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
    ensure!(body == bytes.len() - 9, "pag_body_length_mismatch");
    Ok(PagInfo {
        container_version: bytes[3],
        runtime_version: "4.3.51".into(),
    })
}

pub(crate) fn write_runtime(out: &Path) -> Result<()> {
    let frame = include_str!("ui/pag-frame.html")
        .replace("/*__LIBPAG__*/", include_str!("ui/vendor/libpag/libpag.js"));
    crate::filesystem::write_new(&out.join(RUNTIME_FRAME), frame.as_bytes())?;
    for (name, bytes) in [
        (
            RUNTIME_WASM,
            include_bytes!("ui/vendor/libpag/libpag.wasm").as_slice(),
        ),
        (
            RUNTIME_LICENSE,
            include_bytes!("ui/vendor/libpag/LICENSE.txt").as_slice(),
        ),
    ] {
        crate::filesystem::write_new(&out.join(name), bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_body_length_version_and_compression_before_browser_decoding() {
        let mut bytes = b"PAG\x01\x02\x00\x00\x00U\x00\x00".to_vec();
        assert_eq!(inspect(&bytes).unwrap().container_version, 1);
        bytes[4] = 3;
        assert!(inspect(&bytes).is_err());
        bytes[4] = 2;
        bytes[3] = 2;
        assert!(inspect(&bytes).is_err());
        bytes[3] = 1;
        bytes[8] = b'Z';
        assert!(inspect(&bytes).is_err());
        assert!(inspect(b"PAG").is_err());
    }
}
