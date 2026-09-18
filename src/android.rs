//! Android resource semantics derived from a project-relative path.
//!
//! Files under `res/` are addressed by resource *name* (`R.drawable.name`,
//! `@drawable/name`), never by filename, and the directory qualifiers select a
//! configuration at runtime. Files under `assets/` and `res/raw` are opened by
//! path or as raw streams, so their encoded format is part of the app contract.
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// First API level that decodes lossy WebP.
pub const WEBP_LOSSY_MIN_SDK: u32 = 14;
/// First API level that decodes lossless WebP and WebP with transparency.
pub const WEBP_LOSSLESS_ALPHA_MIN_SDK: u32 = 18;

const RES_TYPES: [&str; 14] = [
    "anim",
    "animator",
    "color",
    "drawable",
    "font",
    "interpolator",
    "layout",
    "menu",
    "mipmap",
    "navigation",
    "raw",
    "transition",
    "values",
    "xml",
];
const DENSITIES: [&str; 8] = [
    "ldpi", "mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi", "nodpi", "anydpi",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidResource {
    /// Module directory (the parent of `src/<set>` or of `res`), project-relative.
    pub module: PathBuf,
    /// Gradle source set such as `main` or `debug`; empty for a bare `res/`.
    pub source_set: String,
    /// `res` or `assets`.
    pub area: String,
    /// Resource type for `res` files, e.g. `drawable`, `mipmap`, `raw`.
    pub res_type: Option<String>,
    /// Directory qualifiers in declaration order, e.g. `["ldrtl", "xxhdpi"]`.
    pub qualifiers: Vec<String>,
    pub density: Option<String>,
    pub rtl: bool,
    /// Platform version qualifier (`-v26`).
    pub api_level: Option<u32>,
    /// Resource name: the basename without extension (and without `.9`).
    pub name: Option<String>,
    pub nine_patch: bool,
}

impl AndroidResource {
    /// Stable description of how this file may be transformed; part of reuse keys.
    pub(crate) fn policy_key(&self, min_sdk: Option<u32>) -> String {
        format!(
            "android:{}:{}:{}:{}",
            self.area,
            self.res_type.as_deref().unwrap_or("-"),
            if self.nine_patch { "9" } else { "-" },
            min_sdk.map_or("unknown".to_string(), |v| v.to_string())
        )
    }

    /// Why this file must keep its encoded format, if it must.
    pub(crate) fn format_lock(&self) -> Option<&'static str> {
        if self.nine_patch {
            Some("android_nine_patch")
        } else if self.res_type.as_deref() == Some("mipmap") {
            Some("android_launcher_icon")
        } else if self.res_type.as_deref() == Some("raw") {
            Some("android_raw_resource")
        } else {
            None
        }
    }
}

fn normal_components(path: &Path) -> Option<Vec<&str>> {
    path.components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect()
}

/// Classify a project-relative file path. Returns `None` for non-Android paths.
pub fn classify(path: &Path) -> Option<AndroidResource> {
    let parts = normal_components(path)?;
    let file = *parts.last()?;
    // `<module>/src/<set>/assets/**`
    if let Some(index) = parts
        .windows(3)
        .position(|w| w[0] == "src" && w[2] == "assets")
        .filter(|index| index + 3 < parts.len())
    {
        return Some(AndroidResource {
            module: parts[..index].iter().collect(),
            source_set: parts[index + 1].to_string(),
            area: "assets".into(),
            res_type: None,
            qualifiers: vec![],
            density: None,
            rtl: false,
            api_level: None,
            name: None,
            nine_patch: false,
        });
    }
    // `**/res/<type>[-qualifiers]/<file>`: resource directories are never nested.
    let count = parts.len();
    if count < 3 || parts[count - 3] != "res" {
        return None;
    }
    let mut segments = parts[count - 2].split('-');
    let res_type = segments.next()?;
    if !RES_TYPES.contains(&res_type) {
        return None;
    }
    let qualifiers: Vec<String> = segments.map(str::to_string).collect();
    let (module, source_set) = if count >= 5 && parts[count - 5] == "src" {
        (
            parts[..count - 5].iter().collect(),
            parts[count - 4].to_string(),
        )
    } else {
        (parts[..count - 3].iter().collect(), String::new())
    };
    let lower = file.to_ascii_lowercase();
    let nine_patch = lower.ends_with(".9.png");
    let stem = file.split('.').next().filter(|stem| !stem.is_empty());
    Some(AndroidResource {
        module,
        source_set,
        area: "res".into(),
        res_type: Some(res_type.to_string()),
        density: qualifiers
            .iter()
            .find(|q| DENSITIES.contains(&q.as_str()) || q.ends_with("dpi"))
            .cloned(),
        rtl: qualifiers.iter().any(|q| q == "ldrtl"),
        api_level: qualifiers
            .iter()
            .find_map(|q| q.strip_prefix('v').and_then(|v| v.parse().ok())),
        qualifiers,
        name: stem.map(str::to_string),
        nine_patch,
    })
}

/// Whether a WebP candidate can be decoded on every API level the app supports.
/// `Err` carries the precise reason shown to the user.
pub(crate) fn webp_compatibility(
    min_sdk: Option<u32>,
    lossless: bool,
    has_alpha: bool,
) -> Result<(), String> {
    let required = if lossless || has_alpha {
        WEBP_LOSSLESS_ALPHA_MIN_SDK
    } else {
        WEBP_LOSSY_MIN_SDK
    };
    match min_sdk {
        Some(level) if level >= required => Ok(()),
        Some(level) => Err(format!(
            "android_min_sdk_{level}_below_webp_requirement_{required}"
        )),
        None => Err("android_min_sdk_unknown".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn res_paths_expose_source_set_qualifiers_and_resource_name() {
        let r = classify(Path::new(
            "app/src/main/res/drawable-ldrtl-xxhdpi-v26/ic_back.webp",
        ))
        .unwrap();
        assert_eq!(r.module, PathBuf::from("app"));
        assert_eq!(r.source_set, "main");
        assert_eq!(r.area, "res");
        assert_eq!(r.res_type.as_deref(), Some("drawable"));
        assert_eq!(r.qualifiers, ["ldrtl", "xxhdpi", "v26"]);
        assert_eq!(r.density.as_deref(), Some("xxhdpi"));
        assert!(r.rtl);
        assert_eq!(r.api_level, Some(26));
        assert_eq!(r.name.as_deref(), Some("ic_back"));
        assert!(!r.nine_patch);
        assert_eq!(r.format_lock(), None);
    }

    #[test]
    fn nine_patch_launcher_and_raw_files_are_format_locked() {
        let nine = classify(Path::new("lib/res/drawable-xhdpi/bubble.9.png")).unwrap();
        assert!(nine.nine_patch);
        assert_eq!(nine.name.as_deref(), Some("bubble"));
        assert_eq!(nine.module, PathBuf::from("lib"));
        assert_eq!(nine.source_set, "");
        assert_eq!(nine.format_lock(), Some("android_nine_patch"));
        let icon = classify(Path::new("app/src/main/res/mipmap-xxxhdpi/ic_launcher.png")).unwrap();
        assert_eq!(icon.format_lock(), Some("android_launcher_icon"));
        let raw = classify(Path::new("app/src/debug/res/raw/intro.png")).unwrap();
        assert_eq!(raw.source_set, "debug");
        assert_eq!(raw.format_lock(), Some("android_raw_resource"));
    }

    #[test]
    fn assets_are_path_addressed_and_other_paths_are_not_android() {
        let asset = classify(Path::new("app/src/main/assets/web/img/logo.png")).unwrap();
        assert_eq!(asset.area, "assets");
        assert_eq!(asset.name, None);
        assert_eq!(asset.format_lock(), None);
        for path in [
            "App/Resources/logo.png",
            "res/logo.png",
            "res/unknown-type/logo.png",
            "app/src/main/res/drawable/nested/logo.png",
            "app/src/main/assets",
            "../res/drawable/a.png",
        ] {
            assert_eq!(classify(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn webp_is_gated_by_min_sdk_and_unknown_is_not_assumed() {
        assert!(webp_compatibility(Some(21), true, true).is_ok());
        assert!(webp_compatibility(Some(14), false, false).is_ok());
        assert_eq!(
            webp_compatibility(Some(16), false, true).unwrap_err(),
            "android_min_sdk_16_below_webp_requirement_18"
        );
        assert_eq!(
            webp_compatibility(Some(13), false, false).unwrap_err(),
            "android_min_sdk_13_below_webp_requirement_14"
        );
        assert_eq!(
            webp_compatibility(None, false, false).unwrap_err(),
            "android_min_sdk_unknown"
        );
    }
}
