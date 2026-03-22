use std::path::Path;

use jot_cache::JotPaths;

use crate::init_templates;

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
    if !global {
        return Err("project-local clean is not implemented yet; use jot clean --global".into());
    }

    let summary = paths.clear_global_cache()?;
    println!(
        "Removed {} JDK entries and {} download entries from {}",
        summary.removed_jdk_entries,
        summary.removed_download_entries,
        paths.root().display()
    );
    Ok(())
}
