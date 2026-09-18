//! Compare two built packages (APK, AAB, IPA or any ZIP) entry by entry.
//!
//! Source-file savings are not package savings: AAPT2 re-compresses PNGs, Xcode
//! compiles asset catalogs, and the archive compresses entries again. This
//! reads the ZIP central directories of two real builds and reports the
//! measured difference.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024 - 1;

#[derive(Debug, Serialize)]
pub struct EntryChange {
    pub name: String,
    pub before_bytes: Option<u64>,
    pub after_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PackageDiff {
    pub before_file_bytes: u64,
    pub after_file_bytes: u64,
    /// Sum of compressed entry sizes: what the package stores.
    pub before_compressed_bytes: u64,
    pub after_compressed_bytes: u64,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    /// Largest differences first.
    pub entries: Vec<EntryChange>,
}

fn le(bytes: &[u8], at: usize, width: usize) -> Result<u64> {
    let slice = bytes.get(at..at + width).context("truncated zip record")?;
    Ok(slice
        .iter()
        .rev()
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte)))
}

/// Entry name → compressed size, from the central directory.
fn entries(bytes: &[u8]) -> Result<BTreeMap<String, u64>> {
    ensure!(bytes.len() >= 22, "not a zip archive");
    let start = bytes.len().saturating_sub(22 + 65_535);
    let eocd = (start..=bytes.len() - 22)
        .rev()
        .find(|&at| bytes[at..at + 4] == *b"PK\x05\x06")
        .context("not a zip archive (no end-of-central-directory record)")?;
    let count = le(bytes, eocd + 10, 2)? as usize;
    let mut offset = le(bytes, eocd + 16, 4)? as usize;
    ensure!(
        count != 0xffff && offset != 0xffff_ffff,
        "zip64 archives are not supported"
    );
    let mut found = BTreeMap::new();
    for _ in 0..count {
        ensure!(
            bytes.get(offset..offset + 4) == Some(b"PK\x01\x02"),
            "corrupt zip central directory"
        );
        let compressed = le(bytes, offset + 20, 4)?;
        let name_length = le(bytes, offset + 28, 2)? as usize;
        let extra = le(bytes, offset + 30, 2)? as usize;
        let comment = le(bytes, offset + 32, 2)? as usize;
        let name = bytes
            .get(offset + 46..offset + 46 + name_length)
            .context("truncated zip entry name")?;
        found.insert(String::from_utf8_lossy(name).into_owned(), compressed);
        offset = offset
            .checked_add(46 + name_length + extra + comment)
            .context("zip offset overflow")?;
    }
    Ok(found)
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let size = fs::metadata(path)
        .with_context(|| format!("reading {}", path.display()))?
        .len();
    if size > MAX_PACKAGE_BYTES {
        bail!("{} is larger than 4 GiB", path.display());
    }
    Ok(fs::read(path)?)
}

pub fn package_diff(before: impl AsRef<Path>, after: impl AsRef<Path>) -> Result<PackageDiff> {
    let (before_bytes, after_bytes) = (read(before.as_ref())?, read(after.as_ref())?);
    let old = entries(&before_bytes).with_context(|| before.as_ref().display().to_string())?;
    let new = entries(&after_bytes).with_context(|| after.as_ref().display().to_string())?;
    let mut changes: Vec<EntryChange> = old
        .keys()
        .chain(new.keys().filter(|name| !old.contains_key(*name)))
        .filter(|name| old.get(*name) != new.get(*name))
        .map(|name| EntryChange {
            name: name.clone(),
            before_bytes: old.get(name).copied(),
            after_bytes: new.get(name).copied(),
        })
        .collect();
    let delta = |c: &EntryChange| {
        (c.after_bytes.unwrap_or(0) as i64 - c.before_bytes.unwrap_or(0) as i64).unsigned_abs()
    };
    changes.sort_by(|a, b| delta(b).cmp(&delta(a)).then(a.name.cmp(&b.name)));
    Ok(PackageDiff {
        before_file_bytes: before_bytes.len() as u64,
        after_file_bytes: after_bytes.len() as u64,
        before_compressed_bytes: old.values().sum(),
        after_compressed_bytes: new.values().sum(),
        added: changes.iter().filter(|c| c.before_bytes.is_none()).count(),
        removed: changes.iter().filter(|c| c.after_bytes.is_none()).count(),
        changed: changes
            .iter()
            .filter(|c| c.before_bytes.is_some() && c.after_bytes.is_some())
            .count(),
        entries: changes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal stored-entry zip writer for fixtures.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut body, mut directory) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let offset = body.len() as u32;
            let header = |signature: &[u8; 4], central: bool| {
                let mut h = signature.to_vec();
                if central {
                    h.extend([20, 0]);
                }
                h.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                h.extend((data.len() as u32).to_le_bytes());
                h.extend((data.len() as u32).to_le_bytes());
                h.extend((name.len() as u16).to_le_bytes());
                h.extend([0, 0]);
                if central {
                    h.extend([0; 10]);
                    h.extend(offset.to_le_bytes());
                }
                h.extend(name.as_bytes());
                h
            };
            body.extend(header(b"PK\x03\x04", false));
            body.extend(*data);
            directory.extend(header(b"PK\x01\x02", true));
        }
        let start = body.len() as u32;
        body.extend(&directory);
        body.extend(b"PK\x05\x06\0\0\0\0");
        body.extend((files.len() as u16).to_le_bytes());
        body.extend((files.len() as u16).to_le_bytes());
        body.extend((directory.len() as u32).to_le_bytes());
        body.extend(start.to_le_bytes());
        body.extend([0, 0]);
        body
    }

    #[test]
    fn reports_changed_added_and_removed_entries_by_measured_size() {
        let dir = tempfile::tempdir().unwrap();
        let before = dir.path().join("before.apk");
        let after = dir.path().join("after.apk");
        fs::write(
            &before,
            zip(&[
                ("res/a.png", &[0; 900]),
                ("res/b.png", &[0; 50]),
                ("classes.dex", &[1; 10]),
            ]),
        )
        .unwrap();
        fs::write(
            &after,
            zip(&[
                ("res/a.webp", &[0; 300]),
                ("res/b.png", &[0; 40]),
                ("classes.dex", &[1; 10]),
            ]),
        )
        .unwrap();
        let diff = package_diff(&before, &after).unwrap();
        assert_eq!((diff.added, diff.removed, diff.changed), (1, 1, 1));
        assert_eq!(
            diff.before_compressed_bytes - diff.after_compressed_bytes,
            610
        );
        assert_eq!(diff.entries[0].name, "res/a.png");
        assert!(diff.before_file_bytes > diff.after_file_bytes);
    }

    #[test]
    fn malformed_archives_are_errors_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.zip");
        fs::write(&good, zip(&[("a", b"x")])).unwrap();
        for (name, bytes) in [
            ("empty", Vec::new()),
            ("text", b"this is not a zip archive at all".to_vec()),
            ("truncated", zip(&[("a", b"x")])[..30].to_vec()),
            ("bad-directory", {
                let mut z = zip(&[("a", b"x")]);
                let at = z.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
                z[at] = b'X';
                z
            }),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            assert!(package_diff(&good, &path).is_err(), "{name}");
        }
        assert!(package_diff(&good, dir.path().join("missing")).is_err());
    }
}
