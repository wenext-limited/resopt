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

/// Create a regenerable report artifact. Unlike `write_new` this does not force
/// the data to disk: an interrupted analysis is discarded as a whole, and on
/// macOS a per-file fsync dominated analysis time (hundreds of seconds summed
/// over a few thousand previews).
pub(crate) fn write_artifact(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

/// Exclusive, crash-safe lock. The operating system releases it when the
/// process exits, so an interrupted run never leaves a project locked; a
/// leftover lock file without a holder is simply reused.
pub(crate) struct ProjectLock {
    file: fs::File,
    path: PathBuf,
}

impl ProjectLock {
    pub(crate) fn acquire(path: PathBuf, busy: &str) -> Result<Self> {
        for _ in 0..8 {
            if let Ok(metadata) = fs::symlink_metadata(&path) {
                ensure!(
                    metadata.file_type().is_file(),
                    "lock path is not a regular file: {}",
                    path.display()
                );
            }
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&path)
                .with_context(|| format!("opening lock {}", path.display()))?;
            match file.try_lock() {
                Ok(()) => {}
                Err(fs::TryLockError::WouldBlock) => bail!("{busy}"),
                Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            }
            let token = format!("pid={} lock={:p}\n", std::process::id(), &file);
            file.set_len(0)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
            // A previous holder may have unlinked this path between our open and
            // lock; only a lock on the file currently at `path` counts.
            // Windows denies reads through a second handle while the lock is
            // held, and keeps an unlinked name reserved until every handle is
            // closed, so the re-check is both impossible and unnecessary there.
            if cfg!(windows) || fs::read(&path).is_ok_and(|current| current == token.as_bytes()) {
                return Ok(Self { file, path });
            }
        }
        bail!("{busy}")
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        // Remove while still holding the lock, then release it.
        let _ = fs::remove_file(&self.path);
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_excludes_a_second_holder_and_recovers_from_a_crashed_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".resopt.lock");
        // A file left behind by a killed process is not a lock.
        fs::write(&path, b"pid=1 (crashed)").unwrap();
        let first = ProjectLock::acquire(path.clone(), "busy").unwrap();
        let error = ProjectLock::acquire(path.clone(), "busy").err().unwrap();
        assert_eq!(error.to_string(), "busy");
        drop(first);
        assert!(!path.exists());
        drop(ProjectLock::acquire(path.clone(), "busy").unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn lock_refuses_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("victim");
        fs::write(&target, b"keep").unwrap();
        let path = dir.path().join(".resopt.lock");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(ProjectLock::acquire(path, "busy").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
    }
}
