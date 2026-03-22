use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use jot_cache::JotPaths;
use jot_config::{find_jot_toml, find_workspace_jot_toml};
use tempfile::NamedTempFile;

pub(crate) fn workspace_project_file(start: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    find_workspace_jot_toml(start)?.ok_or_else(|| {
        "could not find a workspace jot.toml in the current directory or any parent directory"
            .into()
    })
}

pub(crate) fn nearest_project_file(start: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    find_jot_toml(start)?.ok_or_else(|| {
        "could not find jot.toml in the current directory or any parent directory".into()
    })
}

pub(crate) fn write_locked_file(
    paths: &JotPaths,
    output_path: &Path,
    content: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path = paths.locks_dir().join(format!(
        "file-{}.lock",
        sanitize_for_filename(&output_path.to_string_lossy())
    ));
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    lock_file.lock_exclusive()?;

    let parent = output_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no parent directory", output_path.display()),
        )
    })?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content)?;
    temp_file.flush()?;

    if output_path.exists() {
        fs::remove_file(output_path)?;
    }
    temp_file
        .persist(output_path)
        .map_err(|error| error.error)?;

    let _ = lock_file.unlock();
    Ok(())
}

pub(crate) fn sanitize_for_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

pub(crate) fn find_file_named(root: &Path, target_file_name: &str) -> io::Result<Option<PathBuf>> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, target_file_name)? {
                return Ok(Some(found));
            }
            continue;
        }

        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value == target_file_name)
        {
            return Ok(Some(path));
        }
    }

    Ok(None)
}
