//! Per-table checks: coverage per language, empty values and printf argument
//! consistency with the source language.
//!
//! Plural forms are counted for coverage but not compared for arguments:
//! a language may legitimately drop the count from its `one` form, and the
//! parsed model does not say which argument selects the form.
use crate::localization::{LanguageCoverage, LocalizationIssue};
use langcodec::{Entry, EntryStatus, Translation, formats::xcstrings::SHOULD_TRANSLATE_KEY};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Issues kept per file in the report; `issue_counts` still counts all.
pub(crate) const MAX_ISSUES: usize = 500;
const MAX_TEXT_CHARS: usize = 160;

pub(crate) struct TableCheck {
    pub keys: usize,
    pub stale_keys: usize,
    pub languages: Vec<LanguageCoverage>,
    pub issue_counts: BTreeMap<String, usize>,
    /// Sorted by key, then language.
    pub issues: Vec<LocalizationIssue>,
}

/// `key_is_source_text`: in an `.xcstrings` catalog a key without a source
/// value is displayed as itself, so it is not missing in the source language.
pub(crate) fn check(
    source: &str,
    catalogs: &[(String, &langcodec::Resource)],
    key_is_source_text: bool,
) -> TableCheck {
    let mut by_language: BTreeMap<&str, HashMap<&str, &Entry>> = BTreeMap::new();
    for (language, catalog) in catalogs {
        let entries = by_language.entry(language.as_str()).or_default();
        for entry in &catalog.entries {
            entries.insert(entry.id.as_str(), entry);
        }
    }
    let all_keys: BTreeSet<&str> = by_language
        .values()
        .flat_map(|e| e.keys().copied())
        .collect();
    let do_not_translate: BTreeSet<&str> = by_language
        .values()
        .flat_map(|entries| entries.values())
        .filter(|entry| {
            entry.status == EntryStatus::DoNotTranslate
                || entry.custom.get(SHOULD_TRANSLATE_KEY).map(String::as_str) == Some("false")
        })
        .map(|entry| entry.id.as_str())
        .collect();
    let empty_source = HashMap::new();
    let source_entries = by_language.get(source).unwrap_or(&empty_source);
    // A table without its source language compares every key it has.
    let keys: Vec<&str> = all_keys
        .iter()
        .copied()
        .filter(|key| !do_not_translate.contains(key))
        .filter(|key| source_entries.is_empty() || source_entries.contains_key(key))
        .collect();
    let stale_keys = keys
        .iter()
        .filter(|key| {
            by_language.values().any(|entries| {
                entries.get(**key).is_some_and(|entry| {
                    entry.custom.get("extraction_state").map(String::as_str) == Some("stale")
                })
            })
        })
        .count();

    let mut issues = vec![];
    let mut languages = vec![];
    for (language, entries) in &by_language {
        let mut coverage = LanguageCoverage {
            language: (*language).to_string(),
            translated: 0,
            missing: 0,
            empty: 0,
            needs_review: 0,
            extra: 0,
            issues: 0,
        };
        for key in &keys {
            let Some(entry) = entries.get(key) else {
                coverage.missing += 1;
                continue;
            };
            match &entry.value {
                Translation::Singular(value) if value.is_empty() => {
                    coverage.empty += 1;
                    issues.push(issue("empty_value", key, language, vec![], vec![], ""));
                    continue;
                }
                Translation::Singular(value) => {
                    coverage.translated += 1;
                    if *language != source
                        && let Some(Translation::Singular(original)) =
                            source_entries.get(key).map(|e| &e.value)
                        && let Some(kind) = argument_mismatch(original, value)
                    {
                        issues.push(issue(
                            kind,
                            key,
                            language,
                            display(&langcodec::signature(original)),
                            display(&langcodec::signature(value)),
                            value,
                        ));
                    }
                }
                Translation::Plural(plural) if !plural.forms.is_empty() => {
                    coverage.translated += 1;
                }
                Translation::Empty if key_is_source_text && *language == source => {
                    coverage.translated += 1;
                }
                Translation::Plural(_) | Translation::Empty => {
                    coverage.missing += 1;
                    continue;
                }
            }
            if matches!(entry.status, EntryStatus::NeedsReview | EntryStatus::Stale) {
                coverage.needs_review += 1;
            }
        }
        if !source_entries.is_empty() && *language != source {
            coverage.extra = entries
                .keys()
                .filter(|key| {
                    !source_entries.contains_key(*key) && !do_not_translate.contains(*key)
                })
                .count();
        }
        languages.push(coverage);
    }
    languages.sort_by(|a, b| {
        (a.language != source)
            .cmp(&(b.language != source))
            .then_with(|| a.language.cmp(&b.language))
    });
    issues.sort_by(|a, b| (&a.key, &a.language).cmp(&(&b.key, &b.language)));
    for coverage in &mut languages {
        coverage.issues = issues
            .iter()
            .filter(|issue| issue.language == coverage.language)
            .count();
    }
    let mut issue_counts = BTreeMap::new();
    for issue in &issues {
        *issue_counts.entry(issue.kind.clone()).or_insert(0) += 1;
    }
    TableCheck {
        keys: keys.len(),
        stale_keys,
        languages,
        issue_counts,
        issues,
    }
}

fn issue(
    kind: &str,
    key: &str,
    language: &str,
    expected: Vec<String>,
    found: Vec<String>,
    text: &str,
) -> LocalizationIssue {
    let mut shortened: String = text.chars().take(MAX_TEXT_CHARS).collect();
    if text.chars().count() > MAX_TEXT_CHARS {
        shortened.push('…');
    }
    LocalizationIssue {
        kind: kind.into(),
        key: key.into(),
        language: language.into(),
        expected,
        found,
        text: shortened,
    }
}

/// `placeholder_type` when the same arguments are formatted as different
/// types (`%d` against `%@`, which can crash), `placeholder_count` when
/// arguments are missing, added or renumbered. Integer and floating-point
/// variants (`%d`/`%ld`/`%u`, `%f`/`%g`) are treated as equal.
fn argument_mismatch(source: &str, translation: &str) -> Option<&'static str> {
    let expected = arguments(&langcodec::signature(source));
    let found = arguments(&langcodec::signature(translation));
    if expected == found {
        return None;
    }
    let positions = |arguments: &[(String, char)]| -> Vec<String> {
        arguments.iter().map(|(index, _)| index.clone()).collect()
    };
    Some(if positions(&expected) == positions(&found) {
        "placeholder_type"
    } else {
        "placeholder_count"
    })
}

/// `1$d` → (`1`, integer class). Bare tokens keep an empty position.
fn arguments(signature: &[String]) -> Vec<(String, char)> {
    signature
        .iter()
        .map(|token| {
            let (index, kind) = token.rsplit_once('$').unwrap_or(("", token));
            let class = match kind.chars().next().unwrap_or('?') {
                'd' | 'i' | 'u' | 'o' | 'x' => 'd',
                'f' | 'e' | 'g' | 'a' => 'f',
                other => other,
            };
            (index.to_string(), class)
        })
        .collect()
}

fn display(signature: &[String]) -> Vec<String> {
    signature.iter().map(|token| format!("%{token}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use langcodec::{Metadata, Plural, PluralCategory};

    fn catalog(
        language: &str,
        entries: &[(&str, Translation, EntryStatus)],
    ) -> langcodec::Resource {
        langcodec::Resource {
            metadata: Metadata {
                language: language.into(),
                domain: String::new(),
                custom: HashMap::new(),
            },
            entries: entries
                .iter()
                .map(|(id, value, status)| Entry {
                    id: (*id).into(),
                    value: value.clone(),
                    comment: None,
                    status: status.clone(),
                    custom: HashMap::new(),
                })
                .collect(),
        }
    }

    fn text(value: &str) -> Translation {
        Translation::Singular(value.into())
    }

    const DONE: EntryStatus = EntryStatus::Translated;

    #[test]
    fn coverage_counts_translated_missing_empty_review_and_extra_keys() {
        let en = catalog(
            "en",
            &[
                ("a", text("A"), DONE),
                ("b", text("B"), DONE),
                ("c", text("C"), DONE),
                ("brand", text("resopt"), EntryStatus::DoNotTranslate),
            ],
        );
        let fr = catalog(
            "fr",
            &[
                ("a", text("A fr"), EntryStatus::NeedsReview),
                ("b", text(""), EntryStatus::NeedsReview),
                ("old", text("Ancien"), DONE),
            ],
        );
        let result = check("en", &[("fr".into(), &fr), ("en".into(), &en)], false);
        assert_eq!(result.keys, 3);
        assert_eq!(result.languages[0].language, "en");
        assert_eq!(
            result.languages[1],
            LanguageCoverage {
                language: "fr".into(),
                translated: 1,
                missing: 1,
                empty: 1,
                needs_review: 1,
                extra: 1,
                issues: 1,
            }
        );
        assert_eq!(result.issue_counts.get("empty_value"), Some(&1));
    }

    #[test]
    fn argument_types_and_counts_are_compared_with_the_source() {
        let en = catalog(
            "en",
            &[
                ("fans", text("%d fans"), DONE),
                ("gift", text("%@ sent %@"), DONE),
                ("order", text("%1$@ beat %2$@"), DONE),
                ("bonus", text("Get 5% bonus"), DONE),
                ("wide", text("%ld items"), DONE),
            ],
        );
        let zh = catalog(
            "zh-Hant",
            &[
                ("fans", text("%@個粉絲"), DONE),
                ("gift", text("%@ 送出了禮物"), DONE),
                ("order", text("%2$@ 被 %1$@ 擊敗"), DONE),
                ("bonus", text("5% 獎勵"), DONE),
                ("wide", text("%u 件"), DONE),
            ],
        );
        let result = check("en", &[("en".into(), &en), ("zh-Hant".into(), &zh)], false);
        let found: Vec<_> = result
            .issues
            .iter()
            .map(|i| (i.key.as_str(), i.kind.as_str()))
            .collect();
        assert_eq!(
            found,
            [("fans", "placeholder_type"), ("gift", "placeholder_count")]
        );
        assert_eq!(result.issues[0].expected, ["%1$d"]);
        assert_eq!(result.issues[0].found, ["%1$s"]);
        assert_eq!(result.issues[0].text, "%@個粉絲");
    }

    #[test]
    fn plural_forms_count_as_translated_but_are_not_argument_checked() {
        let forms = |one: &str, other: &str| {
            Translation::Plural(Plural {
                id: "items".into(),
                forms: BTreeMap::from([
                    (PluralCategory::One, one.to_string()),
                    (PluralCategory::Other, other.to_string()),
                ]),
            })
        };
        let en = catalog("en", &[("items", forms("One item", "%d items"), DONE)]);
        let ar = catalog("ar", &[("items", forms("عنصر", "%@ عناصر"), DONE)]);
        let result = check("en", &[("en".into(), &en), ("ar".into(), &ar)], false);
        assert_eq!(result.languages[1].translated, 1);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn keys_without_any_value_are_missing_and_long_text_is_shortened() {
        let en = catalog(
            "en",
            &[
                ("new", Translation::Empty, EntryStatus::New),
                ("long", text("%d"), DONE),
            ],
        );
        let long = "x".repeat(400);
        let de = catalog("de", &[("long", text(&long), DONE)]);
        let result = check("en", &[("en".into(), &en), ("de".into(), &de)], false);
        assert_eq!(result.languages[0].missing, 1);
        assert_eq!(result.languages[1].missing, 1);
        assert_eq!(result.issues[0].text.chars().count(), MAX_TEXT_CHARS + 1);
    }

    #[test]
    fn localized_keys_flagged_should_not_translate_are_excluded() {
        let mut en = catalog(
            "en",
            &[("brand", text("WeParty"), DONE), ("a", text("A"), DONE)],
        );
        en.entries[0]
            .custom
            .insert(SHOULD_TRANSLATE_KEY.into(), "false".into());
        let fr = catalog("fr", &[("a", text("A fr"), DONE)]);
        let result = check("en", &[("en".into(), &en), ("fr".into(), &fr)], false);
        assert_eq!(result.keys, 1);
        assert_eq!(result.languages[1].missing, 0);
    }

    #[test]
    fn catalog_keys_without_a_source_value_are_shown_as_the_key() {
        let en = catalog("en", &[("Tap me", Translation::Empty, EntryStatus::New)]);
        let de = catalog("de", &[("Tap me", Translation::Empty, EntryStatus::New)]);
        let catalogs = [("en".into(), &en), ("de".into(), &de)];
        let catalog_result = check("en", &catalogs, true);
        assert_eq!(
            (
                catalog_result.languages[0].translated,
                catalog_result.languages[0].missing
            ),
            (1, 0)
        );
        assert_eq!(catalog_result.languages[1].missing, 1);
        assert_eq!(check("en", &catalogs, false).languages[0].missing, 1);
    }
}
