use resopt::{AnalysisOptions, ResourceAnalysis, analyze};
use std::{fs, path::Path};

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn utf16le(text: &str) -> Vec<u8> {
    [0xFF, 0xFE]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect()
}

fn row<'a>(resources: &'a [ResourceAnalysis], suffix: &str) -> &'a ResourceAnalysis {
    resources
        .iter()
        .find(|r| {
            r.resource
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("no row for {suffix}"))
}

const CATALOG: &str = r#"{
  "sourceLanguage" : "en",
  "strings" : {
    "fans" : { "localizations" : {
      "en" : { "stringUnit" : { "state" : "translated", "value" : "%d fans" } },
      "zh-Hant" : { "stringUnit" : { "state" : "translated", "value" : "%@個粉絲" } }
    } },
    "bonus" : { "localizations" : {
      "en" : { "stringUnit" : { "state" : "translated", "value" : "Get 5% bonus" } },
      "zh-Hant" : { "stringUnit" : { "state" : "translated", "value" : "" } }
    } },
    "removed" : { "extractionState" : "stale", "localizations" : {
      "en" : { "stringUnit" : { "state" : "translated", "value" : "Old" } }
    } }
  },
  "version" : "1.0"
}"#;

fn analyze_project(files: &[(&str, Vec<u8>)]) -> Vec<ResourceAnalysis> {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("project");
    for (path, bytes) in files {
        write(&input, path, bytes);
    }
    let before: Vec<_> = files
        .iter()
        .map(|(path, _)| fs::read(input.join(path)).unwrap())
        .collect();
    let report = analyze(
        &input,
        root.path().join("report"),
        AnalysisOptions {
            probe_only: true,
            ..AnalysisOptions::default()
        },
    )
    .unwrap();
    for ((path, _), bytes) in files.iter().zip(before) {
        assert_eq!(fs::read(input.join(path)).unwrap(), bytes, "{path} changed");
    }
    let json = fs::read(root.path().join("report/analysis.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(
        saved["resources"].as_array().unwrap().len(),
        report.resources.len()
    );
    report.resources
}

#[test]
fn string_catalog_reports_coverage_stale_keys_and_argument_types() {
    let resources = analyze_project(&[("App/Localizable.xcstrings", CATALOG.into())]);
    let catalog = row(&resources, "Localizable.xcstrings");
    assert_eq!(catalog.status, "inspected");
    assert!(catalog.sha256.is_some());
    let info = catalog.localization.as_ref().unwrap();
    assert_eq!(
        (info.format.as_str(), info.table.as_str()),
        ("xcstrings", "Localizable")
    );
    assert_eq!(info.source_language, "en");
    assert_eq!((info.keys, info.stale_keys), (3, 1));
    let languages: Vec<_> = info.languages.iter().map(|l| l.language.as_str()).collect();
    assert_eq!(languages, ["en", "zh-Hant"]);
    let zh = &info.languages[1];
    assert_eq!((zh.translated, zh.missing, zh.empty), (1, 1, 1));
    let issues: Vec<_> = info
        .issues
        .iter()
        .map(|i| (i.kind.as_str(), i.key.as_str(), i.language.as_str()))
        .collect();
    assert_eq!(
        issues,
        [
            ("empty_value", "bonus", "zh-Hant"),
            ("placeholder_type", "fans", "zh-Hant")
        ]
    );
    assert!(info.files.is_empty());
}

#[test]
fn lproj_tables_compare_languages_across_files_including_utf16() {
    let resources = analyze_project(&[
        (
            "App/en.lproj/Localizable.strings",
            b"\"hello\" = \"Hello %@\";\n\"bye\" = \"Bye\";\n".to_vec(),
        ),
        (
            "App/fr.lproj/Localizable.strings",
            utf16le("\"hello\" = \"Bonjour\";\n"),
        ),
        (
            "App/fr.lproj/InfoPlist.strings",
            b"\"CFBundleName\" = \"App\";\n".to_vec(),
        ),
    ]);
    let en = row(&resources, "en.lproj/Localizable.strings");
    let fr = row(&resources, "fr.lproj/Localizable.strings");
    let info = en.localization.as_ref().unwrap();
    assert_eq!(info.source_language, "en");
    assert_eq!(info.files.len(), 2);
    assert_eq!(info.languages[1].language, "fr");
    assert_eq!(info.languages[1].missing, 1);
    assert_eq!(info.issues.len(), 1);
    assert_eq!(info.issues[0].kind, "placeholder_count");
    assert_eq!(fr.localization.as_ref().unwrap().issues, info.issues);
    let plist = row(&resources, "InfoPlist.strings")
        .localization
        .as_ref()
        .unwrap();
    assert_eq!(plist.table, "InfoPlist");
    assert!(plist.files.is_empty());
}

#[test]
fn android_values_group_by_resource_directory_and_skip_non_string_xml() {
    let resources = analyze_project(&[
        (
            "app/src/main/res/values/strings.xml",
            br#"<resources><string name="title">Rooms</string><string name="count">%1$d online</string><string name="brand" translatable="false">WeParty</string></resources>"#.to_vec(),
        ),
        (
            "app/src/main/res/values-zh-rCN/strings.xml",
            r#"<resources><string name="title">房间</string><string name="count">%1$s 在线</string></resources>"#.into(),
        ),
        (
            "app/src/main/res/values-night/strings.xml",
            br#"<resources><string name="title">Rooms</string></resources>"#.to_vec(),
        ),
        (
            "app/src/main/res/values/colors.xml",
            br#"<resources><color name="accent">#FF0000</color></resources>"#.to_vec(),
        ),
    ]);
    let default = row(&resources, "res/values/strings.xml");
    assert_eq!(default.resource.kind, "localization");
    let info = default.localization.as_ref().unwrap();
    assert_eq!(
        (info.format.as_str(), info.source_language.as_str()),
        ("android_strings", "default")
    );
    assert_eq!(info.keys, 2);
    let languages: Vec<_> = info.languages.iter().map(|l| l.language.as_str()).collect();
    assert_eq!(languages, ["default", "zh-CN"]);
    assert_eq!(info.issues[0].kind, "placeholder_type");
    let night = row(&resources, "values-night/strings.xml");
    assert!(night.localization.as_ref().unwrap().files.is_empty());
    let colors = row(&resources, "values/colors.xml");
    assert!(colors.localization.is_none());
    assert_ne!(colors.resource.kind, "localization");
}

#[test]
fn unreadable_catalog_fails_its_row_without_stopping_analysis() {
    let resources = analyze_project(&[
        ("App/Broken.xcstrings", b"{ not json".to_vec()),
        ("App/en.lproj/Main.strings", b"\"a\" = \"A\";\n".to_vec()),
    ]);
    let broken = row(&resources, "Broken.xcstrings");
    assert_eq!(broken.status, "failed");
    assert!(broken.issues[0].starts_with("localization_parse_failed: "));
    assert_eq!(row(&resources, "Main.strings").status, "inspected");
}

#[test]
fn android_folders_normalizing_to_one_language_keep_separate_coverage() {
    let resources = analyze_project(&[
        (
            "app/src/main/res/values/strings.xml",
            br#"<resources><string name="a">A</string><string name="b">B</string></resources>"#
                .to_vec(),
        ),
        (
            "app/src/main/res/values-in/strings.xml",
            br#"<resources><string name="a">A in</string></resources>"#.to_vec(),
        ),
        (
            "app/src/main/res/values-id/strings.xml",
            br#"<resources><string name="b">B id</string></resources>"#.to_vec(),
        ),
    ]);
    let info = row(&resources, "values-in/strings.xml")
        .localization
        .as_ref()
        .unwrap();
    let coverage: Vec<_> = info
        .languages
        .iter()
        .map(|l| (l.language.as_str(), l.translated, l.missing))
        .collect();
    assert_eq!(
        coverage,
        [
            ("default", 2, 0),
            ("id (values-id)", 1, 1),
            ("id (values-in)", 1, 1)
        ]
    );
}

#[test]
fn catalog_keys_marked_do_not_translate_are_not_counted_as_missing() {
    let catalog = r#"{
      "sourceLanguage" : "en",
      "strings" : {
        "brand" : { "shouldTranslate" : false, "localizations" : {
          "en" : { "stringUnit" : { "state" : "translated", "value" : "WeParty" } } } },
        "title" : { "localizations" : {
          "en" : { "stringUnit" : { "state" : "translated", "value" : "Rooms" } },
          "fr" : { "stringUnit" : { "state" : "translated", "value" : "Salons" } } } },
        "Tap me" : { }
      },
      "version" : "1.0"
    }"#;
    let resources = analyze_project(&[("App/Localizable.xcstrings", catalog.into())]);
    let info = row(&resources, "Localizable.xcstrings")
        .localization
        .as_ref()
        .unwrap();
    assert_eq!(info.keys, 2);
    let en = &info.languages[0];
    assert_eq!(
        (en.language.as_str(), en.translated, en.missing),
        ("en", 2, 0)
    );
    let fr = &info.languages[1];
    assert_eq!((fr.translated, fr.missing), (1, 1));
}
