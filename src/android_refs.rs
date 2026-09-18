//! Where an Android resource name is used. A format change keeps the name, so
//! nothing is rewritten; the index tells the reviewer what depends on the file
//! and flags dynamic lookups that static analysis cannot follow.
use crate::{ScanOptions, android::AndroidResource, resources::bounded_read};
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};

const MAX_LISTED_FILES: usize = 20;

#[derive(Debug, Default, Serialize)]
pub(crate) struct Usage {
    /// `@drawable/name` style references in XML files.
    pub xml_references: usize,
    /// `R.drawable.name` style references in Kotlin and Java sources.
    pub code_references: usize,
    /// Files calling `getIdentifier`, which resolves resource names at runtime.
    pub dynamic_lookup_files: usize,
    pub files: Vec<PathBuf>,
}

pub(crate) fn usage(root: &Path, resource: &AndroidResource) -> Result<Usage> {
    let (Some(kind), Some(name)) = (&resource.res_type, &resource.name) else {
        return Ok(Usage::default());
    };
    let name = regex::escape(name);
    let xml = Regex::new(&format!(r"@(?:\+?{kind}|android:{kind})/{name}\b"))?;
    let code = Regex::new(&format!(r"\bR\s*\.\s*{kind}\s*\.\s*{name}\b"))?;
    let filter = crate::scan_options::ScanFilter::new(root, ScanOptions::default())?;
    let mut paths: Vec<_> = filter.paths().collect();
    paths.sort();
    let mut usage = Usage::default();
    for path in paths {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(extension, "xml" | "kt" | "java") || !path.is_file() {
            continue;
        }
        let Some(text) = bounded_read(path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            continue;
        };
        let found = if extension == "xml" {
            let count = xml.find_iter(&text).count();
            usage.xml_references += count;
            count
        } else {
            if text.contains("getIdentifier(") {
                usage.dynamic_lookup_files += 1;
            }
            let count = code.find_iter(&text).count();
            usage.code_references += count;
            count
        };
        if found > 0 && usage.files.len() < MAX_LISTED_FILES {
            usage
                .files
                .push(path.strip_prefix(root).unwrap_or(path).to_path_buf());
        }
    }
    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn counts_xml_code_and_dynamic_lookups_for_the_exact_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let write = |path: &str, text: &str| {
            let file = root.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        };
        write(
            "app/src/main/res/layout/main.xml",
            r#"<ImageView android:src="@drawable/bg_home"/><ImageView android:src="@drawable/bg_home_dark"/>"#,
        );
        write(
            "app/src/main/java/Main.kt",
            "val a = R.drawable.bg_home\nval b = R.drawable.bg_home_dark\nval c = R.mipmap.bg_home",
        );
        write(
            "app/src/main/java/Dynamic.java",
            r#"int id = res.getIdentifier("bg_" + key, "drawable", pkg);"#,
        );
        let resource =
            crate::android::classify(Path::new("app/src/main/res/drawable-xxhdpi/bg_home.png"))
                .unwrap();
        let usage = usage(&root, &resource).unwrap();
        assert_eq!(usage.xml_references, 1);
        assert_eq!(usage.code_references, 1);
        assert_eq!(usage.dynamic_lookup_files, 1);
        assert_eq!(usage.files.len(), 2);
    }
}
