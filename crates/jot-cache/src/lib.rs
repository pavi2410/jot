use directories::BaseDirs;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct JotPaths {
    root: PathBuf,
}

impl JotPaths {
    pub fn new() -> Result<Self, CacheError> {
        let base_dirs = BaseDirs::new().ok_or(CacheError::HomeDirectoryUnavailable)?;
        Ok(Self {
            root: base_dirs.home_dir().join(".jot"),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn jdks_dir(&self) -> PathBuf {
        self.root.join("jdks")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    pub fn ensure_exists(&self) -> Result<(), CacheError> {
        fs::create_dir_all(self.root())?;
        fs::create_dir_all(self.jdks_dir())?;
        fs::create_dir_all(self.downloads_dir())?;
        Ok(())
    }

    pub fn install_dir(&self, vendor: &str, release_name: &str, platform: &str) -> PathBuf {
        self.jdks_dir()
            .join(format!("{vendor}-{release_name}-{platform}"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not locate the current user's home directory")]
    HomeDirectoryUnavailable,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}