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

    pub fn kotlins_dir(&self) -> PathBuf {
        self.root.join("kotlins")
    }

    pub fn kotlin_install_dir(&self, version: &str) -> PathBuf {
        self.kotlins_dir().join(format!("kotlin-{version}"))
    }

    pub fn kotlin_install_lock_path(&self, version: &str) -> PathBuf {
        let safe_version = sanitize_for_filename(version);
        self.locks_dir().join(format!("kotlin-{safe_version}.lock"))
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    pub fn resolve_cache_dir(&self) -> PathBuf {
        self.root.join("resolve-cache")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    pub fn ensure_exists(&self) -> Result<(), CacheError> {
        fs::create_dir_all(self.root())?;
        fs::create_dir_all(self.jdks_dir())?;
        fs::create_dir_all(self.kotlins_dir())?;
        fs::create_dir_all(self.downloads_dir())?;
        fs::create_dir_all(self.resolve_cache_dir())?;
        fs::create_dir_all(self.locks_dir())?;
        Ok(())
    }

    pub fn clear_global_cache(&self) -> Result<CacheCleanupSummary, CacheError> {
        let jdk_entries = count_entries(&self.jdks_dir())?;
        let kotlin_entries = count_entries(&self.kotlins_dir())?;
        let download_entries = count_entries(&self.downloads_dir())?;
        let resolve_cache_entries = count_entries(&self.resolve_cache_dir())?;
        let lock_entries = count_entries(&self.locks_dir())?;

        remove_dir_if_exists(&self.jdks_dir())?;
        remove_dir_if_exists(&self.kotlins_dir())?;
        remove_dir_if_exists(&self.downloads_dir())?;
        remove_dir_if_exists(&self.resolve_cache_dir())?;
        remove_dir_if_exists(&self.locks_dir())?;
        self.ensure_exists()?;

        Ok(CacheCleanupSummary {
            removed_jdk_entries: jdk_entries,
            removed_kotlin_entries: kotlin_entries,
            removed_download_entries: download_entries,
            removed_resolve_cache_entries: resolve_cache_entries,
            removed_lock_entries: lock_entries,
        })
    }

    pub fn install_dir(&self, vendor: &str, release_name: &str, platform: &str) -> PathBuf {
        self.jdks_dir()
            .join(format!("{vendor}-{release_name}-{platform}"))
    }

    pub fn install_lock_path(&self, vendor: &str, version: &str, platform: &str) -> PathBuf {
        let safe_version = sanitize_for_filename(version);
        let safe_platform = sanitize_for_filename(platform);
        self.locks_dir()
            .join(format!("jdk-{vendor}-{safe_version}-{safe_platform}.lock"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheCleanupSummary {
    pub removed_jdk_entries: usize,
    pub removed_kotlin_entries: usize,
    pub removed_download_entries: usize,
    pub removed_resolve_cache_entries: usize,
    pub removed_lock_entries: usize,
}

fn sanitize_for_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

fn count_entries(path: &Path) -> Result<usize, CacheError> {
    if !path.exists() {
        return Ok(0);
    }

    Ok(fs::read_dir(path)?.count())
}

fn remove_dir_if_exists(path: &Path) -> Result<(), CacheError> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not locate the current user's home directory")]
    HomeDirectoryUnavailable,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::JotPaths;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn clear_global_cache_removes_managed_directories_and_recreates_them() {
        let temp = tempdir().expect("tempdir");
        let paths = JotPaths {
            root: temp.path().join(".jot"),
        };
        paths.ensure_exists().expect("ensure paths");

        fs::write(paths.jdks_dir().join("installed.json"), "jdk").expect("write jdk metadata");
        fs::write(paths.kotlins_dir().join("kotlin-2.1.0"), "kotlin").expect("write kotlin dir");
        fs::write(paths.downloads_dir().join("archive.tar.gz"), "jar").expect("write archive");
        fs::write(paths.resolve_cache_dir().join("asset.json"), "cache")
            .expect("write resolve cache");
        fs::write(paths.locks_dir().join("install.lock"), "lock").expect("write lock file");

        let summary = paths.clear_global_cache().expect("clear cache");
        assert_eq!(summary.removed_jdk_entries, 1);
        assert_eq!(summary.removed_kotlin_entries, 1);
        assert_eq!(summary.removed_download_entries, 1);
        assert_eq!(summary.removed_resolve_cache_entries, 1);
        assert_eq!(summary.removed_lock_entries, 1);
        assert!(paths.jdks_dir().is_dir());
        assert!(paths.kotlins_dir().is_dir());
        assert!(paths.downloads_dir().is_dir());
        assert!(paths.resolve_cache_dir().is_dir());
        assert!(paths.locks_dir().is_dir());
        assert_eq!(
            fs::read_dir(paths.jdks_dir()).expect("read jdks").count(),
            0
        );
        assert_eq!(
            fs::read_dir(paths.kotlins_dir())
                .expect("read kotlins")
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(paths.downloads_dir())
                .expect("read downloads")
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(paths.resolve_cache_dir())
                .expect("read resolve cache")
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(paths.locks_dir()).expect("read locks").count(),
            0
        );
    }
}
