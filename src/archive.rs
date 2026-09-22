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
    #[serde(default)]
    pub optimized_images: usize,
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
        optimized_images: 0,
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
        Ok(info) => {
            row.archive = Some(info);
            if !context.options.probe_only
                && !context.control.is_cancelled()
                && resource.conversion_exclusion.is_none()
                && bytes.len() as u64 >= context.options.min_input_bytes
                && let Err(error) = add_candidate(context, resource, index, bytes, &mut row)
            {
                row.status = "failed".into();
                row.candidates.clear();
                row.smallest_candidate = None;
                row.issues.push(format!("{error:#}"));
            }
        }
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

// Keep unmodified entries as their original compressed streams. Only static PNG
// IDAT bytes may change; paths, dimensions, atlas/config payloads and permissions
// are independently verified again at apply time.
fn optimize(bytes: &[u8], context: &Context<'_>, info: &mut ArchiveInfo) -> Result<Vec<u8>> {
    use std::io::Write;
    let (mut zip, _) = open(bytes)?;
    if !info.rewrite_blockers.is_empty() {
        return Ok(bytes.to_vec());
    }
    // Cocos and custom resource manifests may carry digests outside manifest.json.
    for i in 0..zip.len() {
        if info.entries[i].format == "json"
            && !info.entries[i].metadata
            && !info.entries[i].directory
        {
            let data = read_entry(&mut zip, i)?;
            if has_integrity_fields(&data) {
                info.rewrite_blockers.push(format!(
                    "package_integrity_metadata: {}",
                    info.entries[i].path
                ));
            }
        }
    }
    if !info.rewrite_blockers.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    output.set_raw_comment(zip.comment().into())?;
    let policy = crate::optimizer::Policy {
        reductions: false,
        ..context.options.png_policy()
    };
    for i in 0..zip.len() {
        ensure!(
            !context.control.is_cancelled(),
            "archive_analysis_cancelled"
        );
        let entry = &mut info.entries[i];
        let original = if entry.directory {
            vec![]
        } else {
            read_entry(&mut zip, i)?
        };
        let file = zip.by_index(i)?;
        // Preserve extended timestamps/extra fields and per-entry comments verbatim.
        let plain = file.extra_data().is_none_or(|extra| extra.is_empty())
            && file.comment().is_empty()
            && file.unix_mode().is_some();
        let pixels = original.get(16..24).and_then(|data| {
            let width = u32::from_be_bytes(data[..4].try_into().ok()?) as usize;
            let height = u32::from_be_bytes(data[4..].try_into().ok()?) as usize;
            width.checked_mul(height)
        });
        let candidate = if !entry.metadata
            && plain
            && original.starts_with(b"\x89PNG\r\n\x1a\n")
            && pixels.is_some_and(|p| p > 0 && p <= context.options.max_pixels)
        {
            let _lease = context.budget.acquire(pixels.unwrap());
            match crate::optimizer::optimize(&original, &policy) {
                Ok(smaller) if smaller.len() < original.len() => Some(smaller),
                Ok(_) => None,
                Err(error) => {
                    entry
                        .issues
                        .push(format!("archive_image_unchanged: {error:#}"));
                    None
                }
            }
        } else {
            None
        };
        if let Some(smaller) = candidate {
            output.start_file(file.name(), file.options())?;
            output.write_all(&smaller)?;
            info.optimized_images += 1;
        } else {
            output.raw_copy_file(file)?;
        }
    }
    let candidate = output.finish()?.into_inner();
    ensure!(candidate.len() <= MAX_ARCHIVE_BYTES, "archive_output_limit");
    if info.optimized_images == 0 || candidate.len() >= bytes.len() {
        info.optimized_images = 0;
        return Ok(bytes.to_vec());
    }
    verify(bytes, &candidate)?;
    Ok(candidate)
}

fn has_integrity_fields(bytes: &[u8]) -> bool {
    fn inspect(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
                let nonempty = !value.is_null()
                    && value.as_str() != Some("")
                    && !value.as_array().is_some_and(Vec::is_empty)
                    && !value.as_object().is_some_and(serde_json::Map::is_empty);
                (matches!(
                    key.to_ascii_lowercase().as_str(),
                    "md5" | "sha1" | "sha256" | "integrity" | "md5assetsmap"
                ) && nonempty)
                    || (key.eq_ignore_ascii_case("hash")
                        && nonempty
                        && fields
                            .keys()
                            .any(|k| matches!(k.as_str(), "path" | "file" | "filename" | "url")))
                    || inspect(value)
            }),
            serde_json::Value::Array(values) => values.iter().any(inspect),
            _ => false,
        }
    }
    serde_json::from_slice(bytes)
        .map(|v| inspect(&v))
        .unwrap_or(true)
}

pub(crate) fn verify(original: &[u8], candidate: &[u8]) -> Result<()> {
    let (mut before, old) = open(original)?;
    let (mut after, new) = open(candidate)?;
    ensure!(
        old.rewrite_blockers.is_empty(),
        "archive_integrity_contract_requires_publisher"
    );
    ensure!(
        old.entries.len() == new.entries.len() && before.comment() == after.comment(),
        "archive_structure_changed"
    );
    for i in 0..before.len() {
        let a = before.by_index(i)?;
        let b = after.by_index(i)?;
        ensure!(
            a.name_raw() == b.name_raw()
                && a.is_dir() == b.is_dir()
                && a.unix_mode() == b.unix_mode()
                && a.last_modified() == b.last_modified()
                && a.comment() == b.comment()
                && a.extra_data().unwrap_or(&[]) == b.extra_data().unwrap_or(&[]),
            "archive_entry_metadata_changed"
        );
        drop((a, b));
        let a = read_entry(&mut before, i)?;
        let b = read_entry(&mut after, i)?;
        if old.entries[i].format == "json" && !old.entries[i].metadata {
            ensure!(
                !has_integrity_fields(&a),
                "archive_integrity_contract_requires_publisher"
            );
        }
        if a != b {
            ensure!(
                !old.entries[i].metadata && a.starts_with(b"\x89PNG\r\n\x1a\n"),
                "archive_non_image_payload_changed"
            );
            crate::optimizer::verify(&a, &b, false)?;
        }
    }
    Ok(())
}

fn add_candidate(
    context: &Context<'_>,
    resource: &Resource,
    index: usize,
    original: &[u8],
    row: &mut ResourceAnalysis,
) -> Result<()> {
    use crate::filesystem::{hash, write_new};
    let info = row.archive.as_mut().context("missing_archive_info")?;
    let candidate = optimize(original, context, info)?;
    let savings = original.len().saturating_sub(candidate.len()) as u64;
    if savings < context.options.min_savings_bytes.max(1) {
        return Ok(());
    }
    let artifact = PathBuf::from(format!("candidates/{index}-zip-0.zip"));
    let source = PathBuf::from(format!("originals/{index}.zip"));
    let current = crate::filesystem::contained_file(context.root, &resource.path)
        .and_then(|path| crate::resources::bounded_read(&path))?;
    ensure!(
        Some(hash(&current)) == row.sha256,
        "source_changed_during_analysis"
    );
    write_new(&context.out.join(&artifact), &candidate)?;
    write_new(&context.out.join(&source), original)?;
    row.candidates.push(crate::ImageCandidate {
        format: "zip".into(),
        quality: None,
        lossy: false,
        bytes: candidate.len() as u64,
        savings_bytes: savings,
        valid: true,
        rejection: None,
        difference: None,
        artifact: Some(artifact),
        preview: None,
        sha256: Some(hash(&candidate)),
        warnings: vec![],
        notes: vec![format!("archive_pngs_optimized: {}", info.optimized_images)],
    });
    row.original_artifact = Some(source);
    row.smallest_candidate = Some(0);
    row.status = "candidates_available".into();
    Ok(())
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
    fn verification_rejects_changed_atlas_and_injected_digest_contracts() {
        let original = fixture(&[("a.atlas", b"size: 32,32"), ("settings.json", b"{}")]);
        let changed = fixture(&[("a.atlas", b"size: 31,32"), ("settings.json", b"{}")]);
        assert!(verify(&original, &changed).is_err());
        let protected = fixture(&[("settings.json", br#"{"sha256":"abc"}"#)]);
        assert!(verify(&protected, &protected).is_err());
        assert!(!has_integrity_fields(br#"{"md5AssetsMap":{}}"#));
        // Spine's skeleton hash describes the unchanged skeleton, not texture bytes.
        assert!(!has_integrity_fields(br#"{"skeleton":{"hash":"abc"}}"#));
        assert!(has_integrity_fields(br#"{"path":"a.png","hash":"abc"}"#));
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
