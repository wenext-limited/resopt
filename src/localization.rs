//! Translation coverage and placeholder consistency of localization files.
//!
//! Inspection only: every file is parsed from the bytes that were hashed and
//! nothing is written back. Languages are compared within one *table*: an
//! `.xcstrings` catalog on its own, the `<lang>.lproj/<Table>.strings` (or
//! `.stringsdict`) files that share a directory, or the
//! `values[-<locale>]/<name>.xml` files of one Android resource directory.
use crate::{
    Resource,
    analysis::ResourceAnalysis,
    filesystem::{contained_file, hash},
    resources::bounded_read,
};
use anyhow::{Result, anyhow};
use langcodec::{
    FormatType,
    formats::{AndroidStringsFormat, StringsFormat, StringsdictFormat, XcstringsFormat},
    traits::Parser,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) use crate::localization_checks::MAX_ISSUES;

/// Language label of an Android `values/` directory without a locale qualifier.
pub const ANDROID_DEFAULT_LANGUAGE: &str = "default";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalizationInfo {
    /// Table name: the catalog or `.strings` file stem, or the Android file name.
    pub table: String,
    /// `xcstrings`, `strings`, `stringsdict` or `android_strings`.
    pub format: String,
    /// Language every other language is compared with.
    pub source_language: String,
    /// Keys that should be translated (excludes "do not translate" entries).
    pub keys: usize,
    /// Keys Xcode marked stale: no longer found in source code.
    #[serde(default)]
    pub stale_keys: usize,
    /// Source language first, then by language tag.
    pub languages: Vec<LanguageCoverage>,
    /// Every issue in the table, by kind, including issues not listed below.
    pub issue_counts: BTreeMap<String, usize>,
    /// Issues relevant to this file, at most [`MAX_ISSUES`], by key then language.
    pub issues: Vec<LocalizationIssue>,
    /// Files of a multi-file table and the language each one holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<LocalizationFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageCoverage {
    pub language: String,
    /// Keys with a non-empty value.
    pub translated: usize,
    /// Keys without a value in this language.
    pub missing: usize,
    /// Keys whose value is an empty string.
    pub empty: usize,
    /// Translated keys marked "needs review" or "stale".
    pub needs_review: usize,
    /// Keys present only in this language, not in the source language.
    pub extra: usize,
    /// Issues found in this language.
    #[serde(default)]
    pub issues: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizationIssue {
    /// `placeholder_type`, `placeholder_count` or `empty_value`.
    pub kind: String,
    pub key: String,
    pub language: String,
    /// Source-language arguments, as `%1$d`-style canonical placeholders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected: Vec<String>,
    /// Arguments of this language's value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub found: Vec<String>,
    /// This language's value, shortened for display.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizationFile {
    pub path: PathBuf,
    pub language: String,
}

/// Files this module inspects. Android `values` XML qualifies only if it
/// turns out to hold `<string>` or `<plurals>` entries.
pub(crate) fn is_candidate(resource: &Resource) -> bool {
    resource.conversion_exclusion.is_none()
        && (matches!(
            resource.format.as_str(),
            "xcstrings" | "strings" | "stringsdict"
        ) || (resource.extension == "xml"
            && resource.android.as_ref().is_some_and(|android| {
                android.area == "res" && android.res_type.as_deref() == Some("values")
            })))
}

/// One parsed file, keyed by the table it belongs to.
pub(crate) struct ParsedFile {
    pub index: usize,
    pub digest: String,
    pub format: &'static str,
    pub table: String,
    /// Files with equal keys are the languages of one table.
    pub group: String,
    /// Language of a single-language file; `None` for `.xcstrings`.
    pub language: Option<String>,
    pub catalogs: Vec<langcodec::Resource>,
}

/// What reading one candidate produced.
pub(crate) enum Parsed {
    File(ParsedFile),
    /// Not a localization file after all (Android XML without strings,
    /// compiled binary `.strings`); the ordinary inventory row applies.
    NotLocalization,
    /// A valid file the parser does not model (nested `.stringsdict` rules).
    Unsupported(String),
    Failed(String),
}

pub(crate) fn parse(root: &Path, resource: &Resource, index: usize) -> Parsed {
    let bytes = match contained_file(root, &resource.path).and_then(|path| bounded_read(&path)) {
        Ok(bytes) => bytes,
        Err(error) => return Parsed::Failed(format!("{error:#}")),
    };
    if bytes.starts_with(b"bplist") && resource.format == "strings" {
        return Parsed::NotLocalization;
    }
    let digest = hash(&bytes);
    let android = resource.format == "xml";
    match read_catalogs(resource, &bytes) {
        Ok(catalogs) if android && catalogs.iter().all(|c| c.entries.is_empty()) => {
            Parsed::NotLocalization
        }
        Ok(catalogs) => {
            let (format, table, group, language) = identity(resource);
            Parsed::File(ParsedFile {
                index,
                digest,
                format,
                table,
                group,
                language,
                catalogs,
            })
        }
        // `values` XML may hold any resource type; only Apple formats are
        // known to be localization files when they fail to parse.
        Err(_) if android => Parsed::NotLocalization,
        Err(error) if resource.format == "stringsdict" => {
            Parsed::Unsupported(format!("localization_parse_failed: {error}"))
        }
        Err(error) => Parsed::Failed(format!("localization_parse_failed: {error}")),
    }
}

fn read_catalogs(resource: &Resource, bytes: &[u8]) -> Result<Vec<langcodec::Resource>> {
    Ok(match resource.format.as_str() {
        "xcstrings" => XcstringsFormat::from_reader(bytes)?.try_into()?,
        "strings" => vec![StringsFormat::from_reader(bytes)?.into()],
        "stringsdict" => vec![StringsdictFormat::from_reader(bytes)?.into()],
        "xml" => vec![AndroidStringsFormat::from_reader(bytes)?.into()],
        other => return Err(anyhow!("not a localization format: {other}")),
    })
}

/// Format, table name, grouping key and language of one file.
fn identity(resource: &Resource) -> (&'static str, String, String, Option<String>) {
    let path = &resource.path;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = path.parent().unwrap_or(Path::new(""));
    match resource.format.as_str() {
        "xcstrings" => ("xcstrings", stem, key(&["xcstrings", &text(path)]), None),
        format @ ("strings" | "stringsdict") => {
            let format = if format == "strings" {
                "strings"
            } else {
                "stringsdict"
            };
            let lproj = parent
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .and_then(|n| n.strip_suffix(".lproj").map(str::to_string));
            match lproj {
                Some(language) => {
                    let bundle = text(parent.parent().unwrap_or(Path::new("")));
                    (format, stem, key(&[format, &bundle, &name]), Some(language))
                }
                None => (format, stem, key(&[format, &text(path)]), None),
            }
        }
        _ => {
            let values = parent
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let res = text(parent.parent().unwrap_or(Path::new("")));
            let other_qualifiers = non_locale_qualifiers(&values);
            let language =
                langcodec::infer_language_from_path(path, &FormatType::AndroidStrings(None))
                    .ok()
                    .flatten()
                    .filter(|_| values != "values")
                    .unwrap_or_else(|| ANDROID_DEFAULT_LANGUAGE.to_string());
            (
                "android_strings",
                name.clone(),
                key(&["android", &res, &other_qualifiers, &name]),
                Some(language),
            )
        }
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn key(parts: &[&str]) -> String {
    parts.join("\u{0}")
}

/// Qualifiers of a `values-…` directory other than the locale, so that
/// `values-night` and `values-v21` stay separate tables from `values`.
fn non_locale_qualifiers(directory: &str) -> String {
    let mut locale_seen = false;
    let mut region_allowed = false;
    directory
        .split('-')
        .skip(1)
        .filter(|token| {
            let language = !locale_seen
                && (2..=3).contains(&token.len())
                && token.chars().all(|c| c.is_ascii_lowercase())
                && *token != "car";
            let region = region_allowed
                && token.len() >= 3
                && token.starts_with('r')
                && token[1..]
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            let bcp47 = token.starts_with("b+");
            region_allowed = language;
            locale_seen |= language || bcp47;
            !(language || region || bcp47)
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Groups parsed files into tables and returns one report row per file.
pub(crate) fn rows(
    resources: &[Resource],
    files: Vec<ParsedFile>,
) -> Vec<(usize, ResourceAnalysis)> {
    let mut tables: BTreeMap<String, Vec<ParsedFile>> = BTreeMap::new();
    for file in files {
        tables.entry(file.group.clone()).or_default().push(file);
    }
    tables
        .into_values()
        .flat_map(|files| table_rows(resources, files))
        .collect()
}

fn table_rows(
    resources: &[Resource],
    mut files: Vec<ParsedFile>,
) -> Vec<(usize, ResourceAnalysis)> {
    files.sort_by(|a, b| resources[a.index].path.cmp(&resources[b.index].path));
    distinguish_shared_languages(resources, &mut files);
    let mut languages: Vec<(String, &langcodec::Resource)> = vec![];
    for file in &files {
        for catalog in &file.catalogs {
            let language = file
                .language
                .clone()
                .unwrap_or_else(|| catalog.metadata.language.clone());
            languages.push((language, catalog));
        }
    }
    let source = source_language(&files, &languages);
    let key_is_source_text = files.iter().any(|file| file.format == "xcstrings");
    let table = crate::localization_checks::check(&source, &languages, key_is_source_text);
    let listed: Vec<LocalizationFile> = if files.len() > 1 {
        files
            .iter()
            .map(|file| LocalizationFile {
                path: resources[file.index].path.clone(),
                language: file.language.clone().unwrap_or_default(),
            })
            .collect()
    } else {
        vec![]
    };
    files
        .iter()
        .map(|file| {
            let resource = &resources[file.index];
            let mut row = ResourceAnalysis::new(resource, "inspected");
            row.sha256 = Some(file.digest.clone());
            row.resource.kind = "localization".into();
            // The source file (or a single catalog) lists every language's
            // issues; a translation file lists its own.
            let own = file
                .language
                .as_deref()
                .filter(|language| *language != source);
            row.localization = Some(LocalizationInfo {
                table: file.table.clone(),
                format: file.format.into(),
                source_language: source.clone(),
                keys: table.keys,
                stale_keys: table.stale_keys,
                languages: table.languages.clone(),
                issue_counts: table.issue_counts.clone(),
                issues: table
                    .issues
                    .iter()
                    .filter(|issue| own.is_none_or(|language| issue.language == language))
                    .take(MAX_ISSUES)
                    .cloned()
                    .collect(),
                files: listed.clone(),
            });
            (file.index, row)
        })
        .collect()
}

/// Android folders such as `values-in` and `values-id` normalize to one
/// language tag. Merging them would hide either file's gaps, so each keeps
/// its own coverage row, labelled with its folder.
fn distinguish_shared_languages(resources: &[Resource], files: &mut [ParsedFile]) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for file in files.iter() {
        if let Some(language) = &file.language {
            *seen.entry(language.clone()).or_insert(0) += 1;
        }
    }
    for file in files.iter_mut() {
        if let Some(language) = &file.language
            && seen.get(language).is_some_and(|count| *count > 1)
        {
            let folder = resources[file.index]
                .path
                .parent()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            file.language = Some(format!("{language} ({folder})"));
        }
    }
}

/// A catalog names its source language. Across files, the Apple `Base`
/// localization or the Android default `values/` is the source; otherwise
/// English, then the language with the most entries.
fn source_language(files: &[ParsedFile], languages: &[(String, &langcodec::Resource)]) -> String {
    if let Some(declared) = files
        .iter()
        .filter(|file| file.format == "xcstrings")
        .flat_map(|file| &file.catalogs)
        .find_map(|catalog| catalog.metadata.custom.get("source_language"))
        .filter(|language| !language.is_empty())
    {
        return declared.clone();
    }
    for preferred in ["Base", ANDROID_DEFAULT_LANGUAGE, "en"] {
        if languages.iter().any(|(language, _)| language == preferred) {
            return preferred.into();
        }
    }
    languages
        .iter()
        .max_by(|a, b| {
            a.1.entries
                .len()
                .cmp(&b.1.entries.len())
                .then_with(|| b.0.cmp(&a.0))
        })
        .map_or_else(String::new, |(language, _)| language.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_locale_qualifiers_are_not_part_of_the_table() {
        assert_eq!(non_locale_qualifiers("values"), "");
        assert_eq!(non_locale_qualifiers("values-zh-rCN"), "");
        assert_eq!(non_locale_qualifiers("values-b+sr+Latn"), "");
        assert_eq!(non_locale_qualifiers("values-es-night"), "night");
        assert_eq!(non_locale_qualifiers("values-v21"), "v21");
        assert_eq!(non_locale_qualifiers("values-night-v31"), "night-v31");
        assert_eq!(non_locale_qualifiers("values-car"), "car");
    }
}
