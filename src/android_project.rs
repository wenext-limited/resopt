//! Best-effort `minSdk` detection from Gradle build files and version catalogs.
//!
//! Gradle builds are programs, so this only recognizes literal declarations and
//! version-catalog lookups. When nothing is found the level stays unknown and
//! API-gated conversions are blocked until `--android-min-sdk` is given.
use crate::{resources::bounded_read, scan_options::ScanFilter};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::LazyLock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinSdk {
    /// Lowest level declared anywhere in the project.
    pub level: u32,
    /// Project-relative file the level was read from, or `--android-min-sdk`.
    pub source: String,
}

static LITERAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:minSdk|minSdkVersion)\s*(?:=|\s|\()\s*(\d{1,3})\b").unwrap()
});
static CATALOG_LOOKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*(?:minSdk|minSdkVersion)\b[^\n]*?libs\.versions\.([A-Za-z0-9_.]+?)\.get\(\)",
    )
    .unwrap()
});
static CATALOG_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*([A-Za-z0-9_.-]+)\s*=\s*"(\d{1,3})"\s*(?:#.*)?$"#).unwrap()
});

fn catalog_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub(crate) fn detect_min_sdk(root: &Path, filter: &ScanFilter) -> Option<MinSdk> {
    let mut files: Vec<_> = filter
        .paths()
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            name == "build.gradle" || name == "build.gradle.kts" || name == "libs.versions.toml"
        })
        .filter_map(|path| Some((path.strip_prefix(root).ok()?.to_path_buf(), path)))
        .collect();
    files.sort();
    let read = |path: &Path| {
        bounded_read(path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let mut catalog = BTreeMap::new();
    for (relative, path) in &files {
        if relative
            .file_name()
            .is_some_and(|n| n == "libs.versions.toml")
            && let Some(text) = read(path)
        {
            for entry in CATALOG_ENTRY.captures_iter(&text) {
                if let Ok(level) = entry[2].parse::<u32>() {
                    catalog.insert(catalog_key(&entry[1]), (level, relative.clone()));
                }
            }
        }
    }
    let mut found: Vec<MinSdk> = Vec::new();
    for (relative, path) in &files {
        if relative.extension().is_some_and(|e| e == "toml") {
            continue;
        }
        let Some(text) = read(path) else { continue };
        for capture in LITERAL.captures_iter(&text) {
            if let Ok(level) = capture[1].parse() {
                found.push(MinSdk {
                    level,
                    source: relative.to_string_lossy().replace('\\', "/"),
                });
            }
        }
        for capture in CATALOG_LOOKUP.captures_iter(&text) {
            if let Some((level, source)) = catalog.get(&catalog_key(&capture[1])) {
                found.push(MinSdk {
                    level: *level,
                    source: source.to_string_lossy().replace('\\', "/"),
                });
            }
        }
    }
    // Convention plugins hide the assignment; a catalog entry named like minSdk
    // is still the project's declared floor.
    if found.is_empty() {
        for (key, (level, source)) in &catalog {
            if key == "minsdk" || key == "minsdkversion" || key == "androidminsdk" {
                found.push(MinSdk {
                    level: *level,
                    source: source.to_string_lossy().replace('\\', "/"),
                });
            }
        }
    }
    found
        .into_iter()
        .filter(|sdk| (1..=99).contains(&sdk.level))
        .min_by(|a, b| a.level.cmp(&b.level).then(a.source.cmp(&b.source)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn detect(files: &[(&str, &str)]) -> Option<MinSdk> {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        for (path, text) in files {
            let file = root.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let filter = ScanFilter::new(&root, crate::ScanOptions::default()).unwrap();
        detect_min_sdk(&root, &filter)
    }

    #[test]
    fn literal_declarations_use_the_lowest_module_level() {
        let sdk = detect(&[
            (
                "app/build.gradle.kts",
                "android {\n  defaultConfig {\n    minSdk = 24\n  }\n}",
            ),
            (
                "lib/build.gradle",
                "android { defaultConfig {\n minSdkVersion 21\n targetSdkVersion 34 } }",
            ),
        ])
        .unwrap();
        assert_eq!(sdk.level, 21);
        assert_eq!(sdk.source, "lib/build.gradle");
    }

    #[test]
    fn version_catalog_lookups_and_convention_plugin_fallback() {
        let catalog = "[versions]\nagp = \"8.5.0\"\nminSdk = \"23\"\ncompileSdk = \"35\"\n";
        let sdk = detect(&[
            ("gradle/libs.versions.toml", catalog),
            (
                "app/build.gradle.kts",
                "android { defaultConfig {\n minSdk = libs.versions.minSdk.get().toInt()\n} }",
            ),
        ])
        .unwrap();
        assert_eq!(
            (sdk.level, sdk.source.as_str()),
            (23, "gradle/libs.versions.toml")
        );
        let fallback = detect(&[
            ("gradle/libs.versions.toml", catalog),
            (
                "app/build.gradle.kts",
                "plugins { id(\"com.example.android.app\") }",
            ),
        ])
        .unwrap();
        assert_eq!(fallback.level, 23);
    }

    #[test]
    fn unknown_stays_unknown_instead_of_guessing() {
        assert_eq!(
            detect(&[
                (
                    "app/build.gradle",
                    "android { compileSdk 35\n // minSdk 21\n }"
                ),
                (
                    "gradle/libs.versions.toml",
                    "[versions]\ncompileSdk = \"35\"\n"
                ),
            ]),
            None
        );
    }
}
