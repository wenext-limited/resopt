//! Optional AAPT2 validation of Android resources.
//!
//! `aapt2 compile` is what the Android Gradle plugin runs on every file under
//! `res/`. Compiling a candidate proves the build tools accept it (including
//! nine-patch markers) and measures the compiled size, which differs from the
//! source size because AAPT2 re-compresses PNG files on its own.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{fs, path::Path, process::Command};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Compiled {
    /// Size of the compiled `.flat` container for the file.
    pub flat_bytes: u64,
}

/// The SDK's `aapt2`, located once per process.
pub(crate) fn aapt2() -> Option<&'static Path> {
    static FOUND: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    FOUND.get_or_init(crate::tools::find_aapt2).as_deref()
}

/// Compile one resource file as `res/<directory>/<file_name>`.
pub(crate) fn compile(
    aapt2: &Path,
    directory: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<Compiled> {
    let safe = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            && !name.contains("..")
    };
    ensure!(
        safe(directory) && safe(file_name),
        "resource path is not a valid AAPT2 input: {directory}/{file_name}"
    );
    let work = tempfile::tempdir()?;
    let source_directory = work.path().join("res").join(directory);
    fs::create_dir_all(&source_directory)?;
    let source = source_directory.join(file_name);
    fs::write(&source, bytes)?;
    let output = work.path().join("out");
    fs::create_dir(&output)?;
    let result = Command::new(aapt2)
        .arg("compile")
        .arg("-o")
        .arg(&output)
        .arg(&source)
        .output()
        .with_context(|| format!("running {}", aapt2.display()))?;
    ensure!(
        result.status.success(),
        "aapt2 rejected {directory}/{file_name}: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    let flat = fs::read_dir(&output)?
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.path().extension().is_some_and(|e| e == "flat"))
        .context("aapt2 produced no compiled resource")?;
    Ok(Compiled {
        flat_bytes: flat.metadata()?.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_names_are_refused_before_running_anything() {
        let missing = Path::new("/nonexistent/aapt2");
        for (directory, file) in [("../x", "a.png"), ("drawable", "../a.png"), ("", "a.png")] {
            let error = compile(missing, directory, file, b"")
                .unwrap_err()
                .to_string();
            assert!(error.contains("not a valid AAPT2 input"), "{error}");
        }
        assert!(compile(missing, "drawable", "a.png", b"").is_err());
    }

    /// Runs only where the Android SDK build-tools are installed.
    #[test]
    fn aapt2_accepts_webp_and_rejects_a_broken_nine_patch() {
        let Some(aapt2) = crate::tools::find_aapt2() else {
            eprintln!("aapt2 not found; skipping");
            return;
        };
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 8, 8);
            encoder.set_color(png::ColorType::Rgba);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[200; 8 * 8 * 4])
                .unwrap();
        }
        let webp_bytes = webp::Encoder::from_rgba(&[200; 8 * 8 * 4], 8, 8)
            .encode_simple(true, 100.0)
            .unwrap();
        assert!(
            compile(&aapt2, "drawable-xhdpi", "a.png", &png_bytes)
                .unwrap()
                .flat_bytes
                > 0
        );
        assert!(
            compile(&aapt2, "drawable-xhdpi", "a.webp", &webp_bytes)
                .unwrap()
                .flat_bytes
                > 0
        );
        // Solid gray borders are not valid nine-patch markers.
        let error = compile(&aapt2, "drawable-xhdpi", "a.9.png", &png_bytes).unwrap_err();
        assert!(error.to_string().contains("aapt2 rejected"), "{error}");
        // AAPT2 copies WebP files without decoding them, so content validity is
        // resopt's own responsibility (every candidate is decoded and compared).
        assert!(compile(&aapt2, "drawable-xhdpi", "a.png", b"not an image").is_err());
    }
}
