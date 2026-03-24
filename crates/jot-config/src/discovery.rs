use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use crate::errors::ConfigError;
use crate::raw::RawConfig;

/// Walk up the directory tree from `start`, calling `visitor` with each directory.
/// The visitor returns `ControlFlow::Break(T)` to stop early with a result,
/// or `ControlFlow::Continue(())` to keep walking.
fn walk_ancestors<T>(
    start: &Path,
    mut visitor: impl FnMut(&Path) -> ControlFlow<T>,
) -> Result<Option<T>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };

    loop {
        if let ControlFlow::Break(value) = visitor(&current) {
            return Ok(Some(value));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn find_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    walk_ancestors(start, |dir| {
        let candidate = dir.join("jot.toml");
        if candidate.is_file() {
            ControlFlow::Break(candidate)
        } else {
            ControlFlow::Continue(())
        }
    })
}

pub fn find_workspace_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut found = None;
    walk_ancestors(start, |dir| {
        let candidate = dir.join("jot.toml");
        if candidate.is_file() {
            found = Some(candidate);
        }
        ControlFlow::<PathBuf>::Continue(())
    })?;
    Ok(found)
}

pub fn find_workspace_root_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    walk_ancestors(start, |dir| {
        let candidate = dir.join("jot.toml");
        if candidate.is_file() {
            let content = match fs::read_to_string(&candidate) {
                Ok(c) => c,
                Err(_) => return ControlFlow::Continue(()),
            };
            let config: RawConfig = match toml::from_str(&content) {
                Ok(c) => c,
                Err(_) => return ControlFlow::Continue(()),
            };
            if config.workspace.is_some() {
                return ControlFlow::Break(candidate);
            }
        }
        ControlFlow::Continue(())
    })
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
