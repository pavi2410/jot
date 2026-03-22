use std::fs;
use std::path::{Path, PathBuf};

use jot_cache::JotPaths;
use jot_config::load_workspace_build_config;

use crate::init_templates;
use crate::utils::nearest_project_file;

pub(crate) fn handle_init(
    cwd: &Path,
    template: Option<String>,
    group: Option<String>,
    package_name: Option<String>,
    name: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = init_templates::InitOptions {
        template,
        group,
        package_name,
        name,
    };
    let output = init_templates::scaffold(cwd, options)?;
    println!(
        "created {} template at {} ({} files)",
        output.template,
        output.root.display(),
        output.created_files
    );
    Ok(())
}

pub(crate) fn handle_clean(
    global: bool,
    paths: JotPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    if global {
        let summary = paths.clear_global_cache()?;
        println!(
            "Removed {} JDK entries, {} Kotlin entries, and {} download entries from {}",
            summary.removed_jdk_entries,
            summary.removed_kotlin_entries,
            summary.removed_download_entries,
            paths.root().display()
        );
        return Ok(());
    }

    let cwd = std::env::current_dir()?;
    let mut deleted = Vec::new();

    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        for member in workspace.members {
            let target_dir = member.project.project_root.join("target");
            if remove_target_dir(&target_dir)? {
                deleted.push(target_dir);
            }
        }
    } else {
        let project_file = nearest_project_file(&cwd)?;
        let project_root = project_file.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("path {} has no parent directory", project_file.display()),
            )
        })?;
        let target_dir = project_root.join("target");
        if remove_target_dir(&target_dir)? {
            deleted.push(target_dir);
        }
    }

    if deleted.is_empty() {
        println!("no project target directories were removed");
    } else {
        for path in deleted {
            println!("removed {}", path.display());
        }
    }

    Ok(())
}

fn remove_target_dir(path: &PathBuf) -> Result<bool, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(false);
    }

    fs::remove_dir_all(path)?;
    Ok(true)
}
