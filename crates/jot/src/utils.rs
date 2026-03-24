use std::path::{Path, PathBuf};

use jot_cache::JotPaths;
use jot_config::{find_jot_toml, find_workspace_jot_toml};

pub(crate) fn workspace_project_file(start: &Path) -> Result<PathBuf, anyhow::Error> {
    find_workspace_jot_toml(start)?.ok_or_else(|| {
        anyhow::anyhow!(
            "could not find a workspace jot.toml in the current directory or any parent directory"
        )
    })
}

pub(crate) fn nearest_project_file(start: &Path) -> Result<PathBuf, anyhow::Error> {
    find_jot_toml(start)?.ok_or_else(|| {
        anyhow::anyhow!("could not find jot.toml in the current directory or any parent directory")
    })
}

pub(crate) fn write_locked_file(
    paths: &JotPaths,
    output_path: &Path,
    content: &[u8],
) -> anyhow::Result<()> {
    let lock_path = paths.locks_dir().join(format!(
        "file-{}.lock",
        jot_common::sanitize_for_filename(&output_path.to_string_lossy())
    ));
    let _lock = jot_common::FileLock::acquire(&lock_path)?;
    jot_common::atomic_write(output_path, content)?;
    Ok(())
}

pub(crate) fn find_file_named(root: &Path, target_file_name: &str) -> Option<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().is_file() && e.file_name().to_str() == Some(target_file_name))
        .map(|e| e.into_path())
}
