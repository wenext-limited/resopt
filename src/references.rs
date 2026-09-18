//! Migrate statically resolvable references when a loose image changes suffix.
use crate::{
    ScanOptions, filesystem::contained_file, resources::bounded_read, scan_options::ScanFilter,
};
use anyhow::{Context, Result, ensure};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::{
    path::{Component, Path, PathBuf},
    sync::LazyLock,
};

pub(crate) type Edit = (PathBuf, Option<Vec<u8>>, Option<Vec<u8>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReferenceContext {
    pub basename_unique: bool,
    pub lookup_unique: bool,
    #[serde(default)]
    pub other_paths: Vec<PathBuf>,
}

pub(crate) fn supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|v| v.to_str()),
        Some(
            "swift"
                | "m"
                | "mm"
                | "h"
                | "pbxproj"
                | "plist"
                | "storyboard"
                | "xib"
                | "json"
                | "html"
                | "htm"
                | "css"
                | "scss"
                | "js"
                | "ts"
                | "jsx"
                | "tsx"
                | "md"
                | "xml"
                | "yaml"
                | "yml"
        )
    )
}

fn lookup_name(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    stem.strip_suffix("@2x")
        .or_else(|| stem.strip_suffix("@3x"))
        .unwrap_or(&stem)
        .to_ascii_lowercase()
}

pub(crate) fn plan(
    root: &Path,
    source: &Path,
    target: &Path,
    options: ScanOptions,
) -> Result<(Vec<Edit>, ReferenceContext)> {
    let filter = ScanFilter::new(root, options)?;
    ensure!(
        filter.diagnostics.is_empty(),
        "reference scan incomplete: {}",
        filter.diagnostics.join("; ")
    );
    let filename = source
        .file_name()
        .context("missing filename")?
        .to_string_lossy()
        .to_ascii_lowercase();
    let lookup = lookup_name(source);
    let image_extension = |p: &Path| {
        p.extension().and_then(|x| x.to_str()).is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "heic" | "heif" | "webp" | "gif" | "imageset"
            )
        })
    };
    // Count other resources, even if the selected source came from an older report
    // and is now ignored. The selected source always contributes one identity.
    let mut others: Vec<_> = filter
        .paths()
        .filter(|p| p.strip_prefix(root).ok() != Some(source))
        .collect();
    others.sort();
    let context = ReferenceContext {
        other_paths: others
            .iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&filename))
            })
            .map(|p| p.strip_prefix(root).unwrap().to_path_buf())
            .collect(),
        basename_unique: !others.iter().any(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&filename))
        }),
        lookup_unique: !others
            .iter()
            .any(|p| image_extension(p) && lookup_name(p) == lookup),
    };
    let mut paths: Vec<_> = filter
        .paths()
        .filter(|p| supported(p) && p.is_file())
        .collect();
    paths.sort();
    let mut edits = Vec::new();
    for path in paths {
        if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            continue;
        }
        let relative = path.strip_prefix(root)?;
        let before = bounded_read(&contained_file(root, relative)?)?;
        let Ok(text) = std::str::from_utf8(&before) else {
            ensure!(
                !before
                    .windows(filename.len())
                    .any(|w| w.eq_ignore_ascii_case(filename.as_bytes())),
                "binary reference requires manual migration: {}",
                relative.display()
            );
            continue;
        };
        let after = rewrite(relative, text, source, target, &context)
            .with_context(|| format!("references in {}", relative.display()))?;
        if after.as_bytes() != before {
            edits.push((
                relative.to_path_buf(),
                Some(before),
                Some(after.into_bytes()),
            ));
        }
    }
    ensure!(
        edits.len() <= 1000,
        "too many reference files; narrow the scan root"
    );
    Ok((edits, context))
}

fn normalized(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn token(
    value: &str,
    file: &Path,
    source: &Path,
    target: &Path,
    context: &ReferenceContext,
) -> Result<Option<String>> {
    if value.contains("://") || value.starts_with("data:") || value.starts_with('/') {
        return Ok(None);
    }
    let end = value.find(['?', '#']).unwrap_or(value.len());
    let path = &value[..end];
    let filename = source
        .file_name()
        .context("missing filename")?
        .to_string_lossy();
    if !path.ends_with(filename.as_ref()) {
        return Ok(None);
    }
    // Escaped literals require a language-specific parser; do not guess at them.
    ensure!(
        !path.contains('\\'),
        "escaped image reference requires manual migration"
    );
    let candidate = Path::new(path);
    let parent = file.parent().unwrap_or(Path::new(""));
    let local = normalized(&parent.join(candidate));
    if local
        .as_ref()
        .is_some_and(|p| context.other_paths.contains(p))
    {
        return Ok(None);
    }
    let exact =
        local.as_deref() == Some(source) || normalized(candidate).as_deref() == Some(source);
    let suffix = source.ends_with(candidate);
    if !exact && !suffix {
        return Ok(None);
    }
    ensure!(
        exact || context.basename_unique,
        "ambiguous image reference {value:?}; use an explicit relative path"
    );
    let new = candidate.with_extension(target.extension().context("target extension missing")?);
    Ok(Some(format!(
        "{}{}",
        new.to_string_lossy().replace('\\', "/"),
        &value[end..]
    )))
}

static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""([^"\r\n]*)"|'([^'\r\n]*)'|`([^`\r\n]*)`"#).unwrap());
static BUNDLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(forResource\s*:\s*"([^"]+)"\s*,\s*withExtension\s*:\s*")([^"]+)(")"#).unwrap()
});
static NAMED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"((?:UIImage\s*\(\s*named\s*:\s*|Image\s*\(\s*|imageNamed\s*:\s*@)")([^"\r\n]+)(")"#,
    )
    .unwrap()
});
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(url\(\s*|\]\()([^\s"'()]+)(\s*\))"#).unwrap());
static PBX_FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b((?:path|name)\s*=\s*)([^";\r\n]+)(;)"#).unwrap());
static PBX_OBJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)[A-Fa-f0-9]{24}(?: /\*.*?\*/)?\s*=\s*\{[^{}]*\};"#).unwrap()
});
static PBX_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bpath\s*=\s*(?:"([^"]*)"|([^;\s]+))\s*;"#).unwrap());
static PBX_TREE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bsourceTree\s*=\s*(?:"([^"]*)"|([^;\s]+))\s*;"#).unwrap());
static XML_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(<string>)([^<>]*)(</string>)").unwrap());
static XML_IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"((?:\b(?:image|highlightedImage)\s*=\s*|<image\s+name\s*=\s*)")([^"\r\n]+)(")"#)
        .unwrap()
});
static YAML_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^(\s*[^#\r\n:]+:\s*)([^\s"'#]+)([ \t]*(?:#.*)?$)"#).unwrap()
});
static PBX_ISA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bisa\s*=\s*PBXFileReference\s*;").unwrap());
static PBX_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b(lastKnownFileType|explicitFileType)\s*=\s*[^;]+;"#).unwrap());

fn replace_matches(
    text: &str,
    regex: &Regex,
    mut rewrite: impl FnMut(&Captures<'_>) -> Result<String>,
) -> Result<String> {
    let mut out = String::new();
    let mut offset = 0;
    for captures in regex.captures_iter(text) {
        let matched = captures.get(0).unwrap();
        out.push_str(&text[offset..matched.start()]);
        out.push_str(&rewrite(&captures)?);
        offset = matched.end();
    }
    out.push_str(&text[offset..]);
    Ok(out)
}

fn literals(
    text: &str,
    file: &Path,
    source: &Path,
    target: &Path,
    context: &ReferenceContext,
) -> Result<String> {
    replace_matches(text, &QUOTED, |c| {
        let value = (1..=3).find_map(|i| c.get(i)).unwrap();
        let whole = &c[0];
        Ok(
            match token(value.as_str(), file, source, target, context)? {
                Some(new) => format!("{}{new}{}", &whole[..1], &whole[whole.len() - 1..]),
                None => whole.to_string(),
            },
        )
    })
}

pub(crate) fn rewrite(
    file: &Path,
    text: &str,
    source: &Path,
    target: &Path,
    context: &ReferenceContext,
) -> Result<String> {
    ensure!(supported(file), "unsupported reference file");
    if file.extension().is_some_and(|e| e == "pbxproj") {
        return replace_matches(text, &PBX_OBJECT, |c| {
            let original = &c[0];
            if !PBX_ISA.is_match(original) {
                return Ok(original.to_string());
            }
            let Some(path) = PBX_PATH.captures(original) else {
                return Ok(original.to_string());
            };
            let value = path.get(1).or_else(|| path.get(2)).unwrap().as_str();
            if token(value, file, source, target, context)?.is_none() {
                return Ok(original.to_string());
            }
            if let Some(tree) = PBX_TREE.captures(original) {
                let value = tree.get(1).or_else(|| tree.get(2)).unwrap().as_str();
                if !matches!(value, "<group>" | "SOURCE_ROOT") {
                    return Ok(original.to_string());
                }
            }
            let quoted = literals(original, file, source, target, context)?;
            let mut new = replace_matches(&quoted, &PBX_FIELD, |field| {
                let value = field[2].trim();
                Ok(match token(value, file, source, target, context)? {
                    Some(path) => format!("{}{}{}", &field[1], path, &field[3]),
                    None => field[0].to_string(),
                })
            })?;
            if new != original {
                let format = target
                    .extension()
                    .and_then(|e| e.to_str())
                    .context("invalid target extension")?;
                new = PBX_TYPE
                    .replace_all(&new, format!("${{1}} = image.{format};"))
                    .into_owned();
            }
            Ok(new)
        });
    }
    let mut out = text.to_string();
    if file
        .extension()
        .is_some_and(|e| matches!(e.to_str(), Some("swift" | "m" | "mm" | "h")))
    {
        out = replace_matches(&out, &BUNDLE, |c| {
            let joined = format!("{}.{}", &c[2], &c[3]);
            Ok(
                if token(&joined, file, source, target, context)?.is_some() {
                    format!(
                        "{}{}{}",
                        &c[1],
                        target.extension().unwrap().to_string_lossy(),
                        &c[4]
                    )
                } else {
                    c[0].to_string()
                },
            )
        })?;
        out = replace_matches(&out, &NAMED, |c| {
            if Path::new(&c[2]).extension().is_some() {
                return Ok(c[0].to_string());
            }
            let source_lookup = lookup_name(source);
            if c[2].to_ascii_lowercase() != source_lookup {
                return Ok(c[0].to_string());
            }
            ensure!(
                context.lookup_unique,
                "ambiguous extensionless or scaled image lookup {:?}; migrate the image family together",
                &c[2]
            );
            Ok(format!(
                "{}{}.{}{}",
                &c[1],
                &c[2],
                target.extension().unwrap().to_string_lossy(),
                &c[3]
            ))
        })?;
    }
    if file
        .extension()
        .is_some_and(|e| matches!(e.to_str(), Some("storyboard" | "xib")))
    {
        out = replace_matches(&out, &XML_IMAGE, |c| {
            if Path::new(&c[2]).extension().is_some()
                || c[2].to_ascii_lowercase() != lookup_name(source)
            {
                return Ok(c[0].to_string());
            }
            ensure!(
                context.lookup_unique,
                "ambiguous Interface Builder image lookup"
            );
            Ok(format!(
                "{}{}.{}{}",
                &c[1],
                &c[2],
                target.extension().unwrap().to_string_lossy(),
                &c[3]
            ))
        })?;
    }
    if file
        .extension()
        .is_some_and(|e| matches!(e.to_str(), Some("plist" | "xml" | "storyboard" | "xib")))
    {
        out = replace_matches(&out, &XML_TEXT, |c| {
            Ok(match token(&c[2], file, source, target, context)? {
                Some(new) => format!("{}{new}{}", &c[1], &c[3]),
                None => c[0].to_string(),
            })
        })?;
    }
    if file
        .extension()
        .is_some_and(|e| matches!(e.to_str(), Some("yaml" | "yml")))
    {
        out = replace_matches(&out, &YAML_VALUE, |c| {
            Ok(match token(&c[2], file, source, target, context)? {
                Some(new) => format!("{}{new}{}", &c[1], &c[3]),
                None => c[0].to_string(),
            })
        })?;
    }
    out = literals(&out, file, source, target, context)?;
    replace_matches(&out, &URL, |c| {
        Ok(match token(&c[2], file, source, target, context)? {
            Some(new) => format!("{}{new}{}", &c[1], &c[3]),
            None => c[0].to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> ReferenceContext {
        ReferenceContext {
            basename_unique: true,
            lookup_unique: true,
            other_paths: vec![],
        }
    }
    #[test]
    fn exact_paths_relative_paths_and_urls_keep_unrelated_text() {
        let s = Path::new("Resources/banner.png");
        let t = s.with_extension("heic");
        let text = r#"{"image":"../Resources/banner.png","other":"other.png","remote":"https://host/banner.png","cache":"Resources/banner.png?v=2"}"#;
        let out = rewrite(Path::new("Config/images.json"), text, s, &t, &context()).unwrap();
        assert!(out.contains("../Resources/banner.heic"));
        assert!(out.contains("Resources/banner.heic?v=2"));
        assert!(out.contains("https://host/banner.png"));
        assert!(out.contains("other.png"));
        assert_eq!(
            rewrite(
                Path::new("style.css"),
                "url(Resources/banner.png)",
                s,
                &t,
                &context()
            )
            .unwrap(),
            "url(Resources/banner.heic)"
        );
    }
    #[test]
    fn swift_named_and_bundle_calls_migrate_without_changing_unrelated_extensions() {
        let s = Path::new("Resources/banner.png");
        let t = s.with_extension("jpeg");
        let text = r#"let a = UIImage(named: "banner"); let b = Image("banner"); let u = Bundle.main.url(forResource: "banner", withExtension: "png"); let unrelated = "png""#;
        let out = rewrite(Path::new("View.swift"), text, s, &t, &context()).unwrap();
        assert!(out.contains("UIImage(named: \"banner.jpeg\")"));
        assert!(out.contains("Image(\"banner.jpeg\")"));
        assert!(out.contains("withExtension: \"jpeg\""));
        assert!(out.ends_with("unrelated = \"png\""));
    }
    #[test]
    fn pbx_file_reference_updates_path_and_type_only_for_selected_file() {
        let s = Path::new("Resources/banner.png");
        let t = s.with_extension("heic");
        let text = "AAAAAAAAAAAAAAAAAAAAAAAA /* banner.png */ = {isa = PBXFileReference; lastKnownFileType = image.png; path = banner.png; sourceTree = \"<group>\"; };\nBBBBBBBBBBBBBBBBBBBBBBBB = {isa=PBXFileReference; lastKnownFileType = image.png; path = other.png; };";
        let out = rewrite(
            Path::new("App.xcodeproj/project.pbxproj"),
            text,
            s,
            &t,
            &context(),
        )
        .unwrap();
        assert!(out.contains("path = banner.heic"));
        assert!(out.contains("lastKnownFileType = image.heic"));
        assert!(out.contains("lastKnownFileType = image.png; path = other.png"));
    }
    #[test]
    fn ambiguous_basenames_and_scale_families_are_not_guessed() {
        let context = ReferenceContext {
            basename_unique: false,
            lookup_unique: false,
            other_paths: vec![PathBuf::from("Other/banner.png")],
        };
        let s = Path::new("Resources/banner.png");
        let t = s.with_extension("heic");
        assert!(
            rewrite(
                Path::new("View.swift"),
                "UIImage(named: \"banner.png\")",
                s,
                &t,
                &context
            )
            .is_err()
        );
        assert!(
            rewrite(
                Path::new("View.swift"),
                "UIImage(named: \"banner\")",
                s,
                &t,
                &context
            )
            .is_err()
        );
        assert_eq!(
            rewrite(
                Path::new("Other/data.json"),
                "\"banner.png\"",
                s,
                &t,
                &context
            )
            .unwrap(),
            "\"banner.png\""
        );
        assert_eq!(
            rewrite(
                Path::new("data.json"),
                "\"Resources/banner.png\"",
                s,
                &t,
                &context
            )
            .unwrap(),
            "\"Resources/banner.heic\""
        );
    }
    #[test]
    fn xml_yaml_and_external_pbx_references_are_resolved_in_context() {
        let source = Path::new("Resources/banner.png");
        let target = source.with_extension("heic");
        let c = context();
        assert_eq!(
            rewrite(
                Path::new("Config.plist"),
                "<string>banner.png</string>",
                source,
                &target,
                &c
            )
            .unwrap(),
            "<string>banner.heic</string>"
        );
        assert_eq!(
            rewrite(
                Path::new("View.storyboard"),
                r#"<imageView image="banner"/><image name="banner"/>"#,
                source,
                &target,
                &c
            )
            .unwrap(),
            r#"<imageView image="banner.heic"/><image name="banner.heic"/>"#
        );
        assert_eq!(
            rewrite(
                Path::new("config.yaml"),
                "image: Resources/banner.png # keep",
                source,
                &target,
                &c
            )
            .unwrap(),
            "image: Resources/banner.heic # keep"
        );
        for text in [
            r#"AAAAAAAAAAAAAAAAAAAAAAAA = {isa = PBXFileReference; name = banner.png; path = other.png; lastKnownFileType = image.png; };"#,
            r#"AAAAAAAAAAAAAAAAAAAAAAAA = {isa = PBXFileReference; path = banner.png; sourceTree = SDKROOT; lastKnownFileType = image.png; };"#,
        ] {
            assert_eq!(
                rewrite(
                    Path::new("App.xcodeproj/project.pbxproj"),
                    text,
                    source,
                    &target,
                    &c
                )
                .unwrap(),
                text
            );
        }
    }
}
