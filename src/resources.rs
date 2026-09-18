use crate::catalog;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub path: PathBuf,
    pub bytes: u64,
    pub kind: String,
    pub format: String,
    pub extension: String,
    pub extension_mismatch: bool,
    pub origin: String,
    /// Reason the file is excluded from every optimization, if any.
    pub conversion_exclusion: Option<String>,
    /// Reason the file must keep its encoded format; same-format lossless
    /// optimization remains available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_lock: Option<String>,
    /// Android resource semantics for files under `res/` or `assets/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android: Option<crate::android::AndroidResource>,
    /// `optimizable`, `excluded` or `unsupported` on this platform and build.
    #[serde(default)]
    pub support: String,
}

impl Resource {
    #[cfg(test)]
    pub(crate) fn for_tests(path: &str, format: &str) -> Self {
        Self {
            path: path.into(),
            bytes: 0,
            kind: kind(format).into(),
            format: format.into(),
            extension: format.into(),
            extension_mismatch: false,
            origin: "loose_file".into(),
            conversion_exclusion: None,
            format_lock: None,
            android: None,
            support: "optimizable".into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResourceInventory {
    pub schema_version: u32,
    pub root: PathBuf,
    pub catalogs: usize,
    pub assets: Vec<Resource>,
    pub skipped_source_or_tooling_files: usize,
    pub excluded_directories: Vec<PathBuf>,
    pub diagnostics: Vec<String>,
    /// Detected project kinds: `xcode`, `swift_package`, `android`, or `directory`.
    #[serde(default)]
    pub project_kinds: Vec<String>,
    /// Lowest declared Android `minSdk`, when it could be read from build files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android_min_sdk: Option<crate::android_project::MinSdk>,
}

/// Inventory all files other than recognized source/tooling files and build/VCS
/// directories. Unknown files stay visible. This is not build-target resolution.
pub fn inventory(root: impl AsRef<Path>) -> Result<ResourceInventory> {
    inventory_with_options(root, crate::ScanOptions::default())
}

pub fn inventory_with_options(
    root: impl AsRef<Path>,
    options: crate::ScanOptions,
) -> Result<ResourceInventory> {
    let root = fs::canonicalize(root)?;
    let filter = crate::scan_options::ScanFilter::new(&root, options)?;
    let catalogs = catalog::scan_filtered(&root, &filter)?;
    let references: BTreeMap<_, _> = catalogs
        .assets
        .into_iter()
        .map(|a| (a.path.clone(), a))
        .collect();
    let mut report = ResourceInventory {
        schema_version: 3,
        root: catalogs.root,
        catalogs: catalogs.catalogs,
        assets: vec![],
        skipped_source_or_tooling_files: 0,
        excluded_directories: vec![],
        diagnostics: catalogs.diagnostics,
        project_kinds: vec![],
        android_min_sdk: None,
    };
    let mut walk = WalkDir::new(&report.root).follow_links(false).into_iter();
    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.diagnostics.push(error.to_string());
                continue;
            }
        };
        let relative = entry.path().strip_prefix(&report.root)?.to_path_buf();
        if !filter.allows(entry.path()) {
            if entry.file_type().is_dir() {
                report.excluded_directories.push(relative);
                walk.skip_current_dir();
            }
            continue;
        }
        if entry.file_type().is_symlink() {
            report
                .diagnostics
                .push(format!("symlink_skipped: {}", relative.display()));
            continue;
        }
        if entry.file_type().is_dir() {
            // Dependency source directories (Pods/Carthage/node_modules) remain
            // visible in all-resource mode, unlike the legacy catalog-only plan.
            let name = entry.file_name().to_string_lossy();
            if entry.depth() > 0
                && ((catalog::excluded(&name)
                    && !matches!(&*name, "Pods" | "Carthage" | "node_modules"))
                    || matches!(
                        &*name,
                        ".github" | ".codex" | ".claude" | ".agents" | ".idea" | ".vscode"
                    ))
            {
                report.excluded_directories.push(relative);
                walk.skip_current_dir();
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let in_catalog = relative.components().any(|part| {
            Path::new(part.as_os_str())
                .extension()
                .is_some_and(|e| e == "xcassets")
        });
        let in_resource_directory = relative.components().any(|part| {
            part.as_os_str().to_str().is_some_and(|name| {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "resources" | "resource" | "assets" | "res"
                )
            })
        });
        if !in_catalog
            && !in_resource_directory
            && is_source_or_tooling(entry.file_name().to_str().unwrap_or(""), &extension)
        {
            report.skipped_source_or_tooling_files += 1;
            continue;
        }
        let mut header = [0_u8; 512];
        let read = match fs::File::open(entry.path()).and_then(|mut f| f.read(&mut header)) {
            Ok(n) => n,
            Err(error) => {
                report
                    .diagnostics
                    .push(format!("{}: {error}", relative.display()));
                continue;
            }
        };
        let format = actual_format(&header[..read])
            .unwrap_or_else(|| extension_format(&extension))
            .to_string();
        let kind = kind(&format).to_string();
        let extension_mismatch =
            matches!(kind.as_str(), "image") && extension_format(&extension) != format;
        let referenced = references.get(&relative);
        let origin = if referenced.is_some() {
            "catalog_rendition"
        } else if in_catalog {
            "catalog_file"
        } else {
            "loose_file"
        }
        .to_string();
        let android = crate::android::classify(&relative);
        let conversion_exclusion = if relative.components().any(|p| {
            Path::new(p.as_os_str())
                .extension()
                .is_some_and(|e| e == "appiconset")
        }) {
            Some("app_icon".into())
        } else {
            referenced
                .and_then(|a| a.reason.as_ref())
                .filter(|r| r.as_str() == "resizing")
                .cloned()
        };
        let format_lock = android
            .as_ref()
            .and_then(|a| a.format_lock())
            .map(str::to_string);
        let support = if conversion_exclusion.is_some() {
            "excluded"
        } else if crate::capabilities::can_optimize(&kind, &format) {
            "optimizable"
        } else {
            "unsupported"
        }
        .to_string();
        report.assets.push(Resource {
            path: relative,
            bytes: entry.metadata()?.len(),
            kind,
            format,
            extension,
            extension_mismatch,
            origin,
            conversion_exclusion,
            format_lock,
            android,
            support,
        });
    }
    report.project_kinds = project_kinds(&filter, &report);
    if report.project_kinds.iter().any(|kind| kind == "android") {
        report.android_min_sdk = crate::android_project::detect_min_sdk(&report.root, &filter);
    }
    report.assets.sort_by(|a, b| a.path.cmp(&b.path));
    report.excluded_directories.sort();
    report.diagnostics.sort();
    Ok(report)
}

pub(crate) fn actual_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("jpeg");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return Some("tiff");
    }
    if bytes.starts_with(b"BM") {
        return Some("bmp");
    }
    if bytes.starts_with(b"%PDF-") {
        return Some("pdf");
    }
    if bytes.get(4..8) == Some(b"ftyp") && bytes.len() >= 16 {
        let size = u32::from_be_bytes(bytes[..4].try_into().ok()?) as usize;
        if size < 16 {
            return None;
        }
        let brands = &bytes[8..size.min(bytes.len())];
        if brands
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| matches!(b, b"avif" | b"avis"))
        {
            return Some("avif");
        }
        if brands
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| matches!(b, b"heic" | b"heix" | b"hevc" | b"hevx"))
        {
            return Some("heic");
        }
        if brands
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| matches!(b, b"mif1" | b"msf1"))
        {
            return Some("heif");
        }
    }
    None
}

fn extension_format(extension: &str) -> &str {
    match extension {
        "jpg" | "jpe" => "jpeg",
        "tif" => "tiff",
        "heics" => "heic",
        "" => "unknown",
        other => other,
    }
}

fn kind(format: &str) -> &'static str {
    match format {
        "png" | "jpeg" | "heic" | "heif" | "webp" | "gif" | "tiff" | "bmp" | "avif" | "jxl"
        | "ico" | "icns" | "psd" => "image",
        "svg" | "pdf" => "vector",
        "mp4" | "mov" | "m4v" | "webm" | "avi" => "video",
        "mp3" | "m4a" | "aac" | "wav" | "ogg" | "caf" | "flac" | "aiff" => "audio",
        "svga" | "vap" | "tcmp4" | "lottie" | "pag" => "animation",
        "ttf" | "otf" | "woff" | "woff2" => "font",
        "zip" | "gz" | "br" | "7z" | "rar" | "tar" => "archive",
        "xcstrings" | "strings" | "stringsdict" => "localization",
        "json" | "yaml" | "yml" | "toml" | "xml" | "plist" | "html" | "css" | "js" | "bin"
        | "dat" | "txt" | "csv" | "db" | "sqlite" => "data",
        _ => "unclassified",
    }
}

fn is_source_or_tooling(name: &str, extension: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "LICENSE" | "Makefile" | "Podfile" | "Gemfile" | "Rakefile"
        )
        || matches!(
            extension,
            "swift"
                | "rs"
                | "m"
                | "mm"
                | "h"
                | "c"
                | "cc"
                | "cpp"
                | "hpp"
                | "kt"
                | "java"
                | "py"
                | "pyc"
                | "pyo"
                | "sh"
                | "rb"
                | "toml"
                | "lock"
                | "md"
                | "yml"
                | "yaml"
                | "pbxproj"
                | "xcscheme"
                | "xcworkspacedata"
                | "xcuserstate"
                | "xcconfig"
                | "entitlements"
                | "resolved"
        )
}

pub(crate) fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64 * 1024 * 1024, "input_exceeds_64_mib");
    Ok(bytes)
}

/// Project kinds present under the scan root. A directory can hold several.
fn project_kinds(
    filter: &crate::scan_options::ScanFilter,
    report: &ResourceInventory,
) -> Vec<String> {
    let mut kinds = std::collections::BTreeSet::new();
    for path in filter.paths() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace") {
            kinds.insert("xcode");
        } else if name == "Package.swift" {
            kinds.insert("swift_package");
        } else if name == "AndroidManifest.xml" {
            kinds.insert("android");
        }
    }
    if report.catalogs > 0 && !kinds.contains("swift_package") {
        kinds.insert("xcode");
    }
    if report.assets.iter().any(|a| a.android.is_some()) {
        kinds.insert("android");
    }
    if kinds.is_empty() {
        kinds.insert("directory");
    }
    kinds.into_iter().map(str::to_string).collect()
}
