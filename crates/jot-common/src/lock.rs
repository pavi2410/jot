use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

/// An exclusive file lock backed by `fs2`.
///
/// The lock is released automatically when the value is dropped.
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// Acquire an exclusive lock on `path`, creating the file if it does not exist.
    pub fn acquire(path: &Path) -> Result<Self, io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }

    /// Return a reference to the underlying file (useful for writing lock metadata).
    pub fn file(&self) -> &File {
        &self.file
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
