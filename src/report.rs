use crate::{
    AnalysisReport,
    filesystem::{contained_file, replace, write_new},
    resources::bounded_read,
};
use anyhow::{Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Refresh presentation only. Does not encode, change JSON, or touch artifacts.
pub fn refresh_report(directory: impl AsRef<Path>) -> Result<PathBuf> {
    let directory = fs::canonicalize(directory)?;
    let data = contained_file(&directory, Path::new("analysis.json"))?;
    let report: AnalysisReport = serde_json::from_slice(&bounded_read(&data)?)?;
    ensure!(
        report.schema_version == 1,
        "unsupported analysis schema version"
    );
    let html = render_html(&report)?;
    let output = directory.join("report.html");
    if fs::symlink_metadata(&output).is_ok() {
        contained_file(&directory, Path::new("report.html"))?;
        replace(&output, html.as_bytes())?;
    } else {
        write_new(&output, html.as_bytes())?;
    }
    Ok(output)
}

pub(crate) fn render_html(report: &AnalysisReport) -> Result<String> {
    let payload = serde_json::to_string(&serde_json::json!({
        "root": report.root,
        "options": report.options,
        "resources": report.resources,
        "savings": report.potential_source_bytes_saved,
        "diagnostics": report.inventory.diagnostics,
        "excludedDirectories": report.inventory.excluded_directories,
    }))?;
    // An inert JSON script still ends at a literal </script>. Escape HTML
    // delimiters before embedding data; UI code uses textContent for filenames.
    let safe = payload
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(include_str!("report.html").replacen("__RESOPT_DATA__", &safe, 1))
}
