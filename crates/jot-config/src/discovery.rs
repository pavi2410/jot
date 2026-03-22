use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::ConfigError;
use crate::raw::RawConfig;

pub fn find_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };

    loop {
        let candidate = current.join("jot.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn find_workspace_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };
    let mut found = None;

    loop {
        let candidate = current.join("jot.toml");
        if candidate.is_file() {
            found = Some(candidate);
        }

        if !current.pop() {
            return Ok(found);
        }
    }
}

pub fn find_workspace_root_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };

    loop {
        let candidate = current.join("jot.toml");
        if candidate.is_file() {
            let content = fs::read_to_string(&candidate)?;
            let config: RawConfig = toml::from_str(&content)?;
            if config.workspace.is_some() {
                return Ok(Some(candidate));
            }
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn read_toolchain_request(
    start: &Path,
) -> Result<Option<jot_toolchain::JavaToolchainRequest>, ConfigError> {
    let Some(path) = find_jot_toml(start)? else {
        return Ok(None);
    };

    let content = fs::read_to_string(&path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let inherited = crate::inherited_workspace_context(path.parent().unwrap_or(start))?;
    Ok(crate::parse_toolchain_request(config.toolchains.as_ref())
        .or(inherited.and_then(|ctx| ctx.toolchain)))
}
