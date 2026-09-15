use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub(crate) fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Reject symlinks and path traversal, even when a link remains inside root.
pub(crate) fn contained_file(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(!relative.as_os_str().is_empty(), "empty relative path");
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!("unsafe relative path: {}", relative.display());
        };
        path.push(component);
        let metadata =
            fs::symlink_metadata(&path).with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "symlink refused: {}",
            path.display()
        );
    }
    ensure!(path.is_file(), "not a regular file: {}", path.display());
    Ok(path)
}

pub(crate) fn read_verified(path: &Path, expected: &str) -> Result<Vec<u8>> {
    let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    ensure!(
        hash(&data) == expected,
        "hash mismatch (stale or modified): {}",
        path.display()
    );
    Ok(data)
}

/// A file replacement is atomic on the local filesystem. A batch is not.
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("file has no parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.as_file()
        .set_permissions(fs::metadata(path)?.permissions())?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
