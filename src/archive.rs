//! Bounded ZIP inspection. Archive names are data, never filesystem destinations.
use crate::{Resource, ResourceAnalysis, analyze_image::Context, filesystem::write_artifact};
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PREVIEWS: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub path: String,
    pub bytes: u64,
    pub compressed_bytes: u64,
    pub format: String,
    pub kind: String,
    pub directory: bool,
    pub metadata: bool,
    pub preview: Option<PathBuf>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveInfo {
    pub entries: Vec<ArchiveEntry>,
    pub expanded_bytes: u64,
    pub compressed_bytes: u64,
    pub preview_limit: usize,
    /// Rewriting must not invalidate external package integrity contracts.
    pub rewrite_blockers: Vec<String>,
}

fn safe_name(name: &str) -> bool {
    let path = name.strip_suffix('/').unwrap_or(name);
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', ':', '\0'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

fn metadata(name: &str) -> bool {
    name.split('/')
        .any(|part| part == "__MACOSX" || part == ".DS_Store" || part.starts_with("._"))
}

fn open(bytes: &[u8]) -> Result<(ZipArchive<Cursor<&[u8]>>, ArchiveInfo)> {
    ensure!(bytes.len() <= MAX_ARCHIVE_BYTES, "archive_input_limit");
    // Bound the central-directory allocation before the ZIP library parses it.
    ensure!(bytes.len() >= 22, "invalid_zip");
    let end = (bytes.len().saturating_sub(65_557)..=bytes.len() - 22)
        .rev()
        .find(|&i| {
            bytes.get(i..i + 4) == Some(b"PK\x05\x06")
                && i + 22 + u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as usize
                    == bytes.len()
        })
        .context("archive_end_record_missing")?;
    let u16_at = |i| u16::from_le_bytes([bytes[end + i], bytes[end + i + 1]]);
    let count = u16_at(10) as usize;
    ensure!(
        u16_at(4) == 0 && u16_at(6) == 0 && u16_at(8) as usize == count,
        "archive_multidisk_not_supported"
    );
    ensure!(count <= MAX_ENTRIES, "archive_entry_count_limit");
    let mut zip = ZipArchive::new(Cursor::new(bytes)).context("invalid_zip")?;
    ensure!(zip.len() == count, "archive_duplicate_or_zip64_directory");
    ensure!(zip.len() <= MAX_ENTRIES, "archive_entry_count_limit");
    let mut info = ArchiveInfo {
        entries: vec![],
        expanded_bytes: 0,
        compressed_bytes: 0,
        preview_limit: MAX_PREVIEWS,
        rewrite_blockers: vec![],
    };
    let mut names = BTreeSet::new();
    for i in 0..zip.len() {
        let file = zip.by_index(i).context("archive_entry_unreadable")?;
        let name = std::str::from_utf8(file.name_raw()).context("archive_non_utf8_path")?;
        ensure!(safe_name(name), "archive_unsafe_path");
        ensure!(
            names.insert(name.trim_end_matches('/').to_lowercase()),
            "archive_duplicate_path"
        );
        ensure!(!file.is_symlink(), "archive_symlink");
        ensure!(!file.encrypted(), "archive_encrypted");
        ensure!(file.size() <= MAX_ENTRY_BYTES, "archive_entry_size_limit");
        info.expanded_bytes = info
            .expanded_bytes
            .checked_add(file.size())
            .context("archive_size_overflow")?;
        ensure!(
            info.expanded_bytes <= MAX_EXPANDED_BYTES,
            "archive_expansion_limit"
        );
        info.compressed_bytes += file.compressed_size();
        let lower = name.to_ascii_lowercase();
        let base = lower.rsplit('/').next().unwrap_or("");
        // These require a package publisher, not an in-place byte optimizer.
        if matches!(base, "manifest.json" | "pack-info.json" | "sniff.json")
            || lower.starts_with("meta-inf/")
            || lower.contains("_codesignature/")
            || lower.ends_with(".sig")
            || lower.ends_with(".signature")
        {
            info.rewrite_blockers
                .push(format!("package_integrity_metadata: {name}"));
        }
        let extension = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let format = match extension.as_str() {
            "jpg" | "jpe" => "jpeg".into(),
            "tif" => "tiff".into(),
            _ => extension,
        };
        info.entries.push(ArchiveEntry {
            path: name.into(),
            bytes: file.size(),
            compressed_bytes: file.compressed_size(),
            kind: crate::resources::kind(&format).into(),
            format,
            directory: file.is_dir(),
            metadata: metadata(name),
            preview: None,
            issues: vec![],
        });
    }
    // Even though nothing is extracted, reject file/directory aliasing as ambiguous.
    for entry in &info.entries {
        if !entry.directory {
            let prefix = format!("{}/", entry.path.to_lowercase());
            ensure!(
                !names.iter().any(|name| name.starts_with(&prefix)),
                "archive_path_conflict"
            );
        }
    }
    Ok((zip, info))
}

fn read_entry(zip: &mut ZipArchive<Cursor<&[u8]>>, index: usize) -> Result<Vec<u8>> {
    let file = zip.by_index(index)?;
    let size = file.size();
    ensure!(size <= MAX_ENTRY_BYTES, "archive_entry_size_limit");
    let mut bytes = Vec::new();
    file.take(size + 1)
        .read_to_end(&mut bytes)
        .context("archive_entry_crc_or_data_error")?;
    ensure!(bytes.len() as u64 == size, "archive_entry_size_mismatch");
    Ok(bytes)
}

pub(crate) fn analyze(
    context: &Context<'_>,
    resource: &Resource,
    index: usize,
    bytes: &[u8],
    digest: &str,
) -> ResourceAnalysis {
    let mut row = ResourceAnalysis::new(resource, "inspected");
    row.sha256 = Some(digest.into());
    match inspect(context, index, bytes) {
        Ok(info) => row.archive = Some(info),
        Err(error) => {
            row.status = "failed".into();
            row.issues.push(format!("{error:#}"));
        }
    }
    if context.control.is_cancelled() {
        row.status = "not_analyzed".into();
    }
    row
}

fn inspect(context: &Context<'_>, index: usize, bytes: &[u8]) -> Result<ArchiveInfo> {
    let (mut zip, mut info) = open(bytes)?;
    let mut images: Vec<_> = info
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.directory && !e.metadata && crate::capabilities::decodable(&e.format))
        .map(|(i, e)| (i, e.bytes))
        .collect();
    images.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    for (i, _) in images.into_iter().take(MAX_PREVIEWS) {
        if context.control.is_cancelled() {
            break;
        }
        let attempt = (|| -> Result<PathBuf> {
            let bytes = read_entry(&mut zip, i)?;
            let _lease = context.budget.acquire(context.options.max_pixels);
            let decoded = crate::image_backend::decode(&bytes, context.options.max_pixels)?;
            let png = crate::image_backend::preview(&decoded)?;
            let preview = PathBuf::from(format!("previews/{index}-zip-{i}.png"));
            write_artifact(&context.out.join(&preview), &png)?;
            Ok(preview)
        })();
        match attempt {
            Ok(path) => info.entries[i].preview = Some(path),
            Err(error) => info.entries[i]
                .issues
                .push(format!("preview_unavailable: {error:#}")),
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn inspects_contents_without_extracting_and_flags_manifest_contracts() {
        let bytes = fixture(&[
            ("assets/texture.png", b"png"),
            ("manifest.json", b"{}"),
            ("__MACOSX/._texture.png", b"metadata"),
        ]);
        let (mut zip, info) = open(&bytes).unwrap();
        assert_eq!(info.entries.len(), 3);
        assert_eq!(info.expanded_bytes, 13);
        assert_eq!(read_entry(&mut zip, 0).unwrap(), b"png");
        assert!(info.entries[2].metadata);
        assert_eq!(info.rewrite_blockers.len(), 1);
    }

    #[test]
    fn rejects_unsafe_ambiguous_and_oversized_archives() {
        for name in ["../escape.png", "/absolute.png", "a/../b", "C:/x", "a\\b"] {
            assert!(open(&fixture(&[(name, b"x")])).is_err(), "{name}");
        }
        assert!(open(&fixture(&[("A.png", b"x"), ("a.png", b"y")])).is_err());
        assert!(open(&fixture(&[("a", b"x"), ("a/b", b"y")])).is_err());
        let mut bytes = fixture(&[("a.png", b"x")]);
        let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        bytes[central + 24..central + 28]
            .copy_from_slice(&((MAX_ENTRY_BYTES + 1) as u32).to_le_bytes());
        assert!(open(&bytes).is_err());
    }
}
