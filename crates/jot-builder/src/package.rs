use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use jot_toolchain::InstalledJdk;
use zip::ZipArchive;

use crate::errors::BuildError;

pub(crate) fn package_jar(
    installed_jdk: &InstalledJdk,
    classes_dir: &Path,
    jar_path: &Path,
    main_class: Option<&str>,
) -> Result<(), BuildError> {
    if let Some(parent) = jar_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut command = Command::new(installed_jdk.jar_binary());
    command.arg("--create").arg("--file").arg(jar_path);
    if let Some(main_class) = main_class {
        command.arg("--main-class").arg(main_class);
    }
    command.arg("-C").arg(classes_dir).arg(".");

    let output = command.output()?;
    if !output.status.success() {
        return Err(BuildError::CommandFailed {
            tool: "jar",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(())
}

pub(crate) fn build_fat_jar(
    installed_jdk: &InstalledJdk,
    dependency_jars: &[PathBuf],
    classes_dir: &Path,
    fat_jar_path: &Path,
    main_class: &str,
) -> Result<Vec<String>, BuildError> {
    let staging_root = fat_jar_path
        .parent()
        .ok_or_else(|| BuildError::InvalidFatJarPath(fat_jar_path.to_path_buf()))?
        .join(".fatjar-staging");
    super::prepare_directory(&staging_root)?;

    let mut services = BTreeMap::<String, Vec<String>>::new();
    let mut warnings = Vec::<String>::new();

    for jar_path in dependency_jars {
        extract_jar_into_staging(jar_path, &staging_root, &mut services, &mut warnings)?;
    }

    overlay_directory_into_staging(classes_dir, &staging_root, &mut services)?;
    write_merged_service_files(&staging_root, &services)?;
    package_jar(installed_jdk, &staging_root, fat_jar_path, Some(main_class))?;
    fs::remove_dir_all(&staging_root)?;

    Ok(warnings)
}

fn extract_jar_into_staging(
    jar_path: &Path,
    staging_root: &Path,
    services: &mut BTreeMap<String, Vec<String>>,
    warnings: &mut Vec<String>,
) -> Result<(), BuildError> {
    let file = fs::File::open(jar_path)?;
    let mut archive = ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if !entry.is_file() {
            continue;
        }

        let name = entry.name().replace('\\', "/");
        if !is_safe_zip_path(&name) || should_skip_jar_entry(&name) {
            continue;
        }

        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;

        if is_service_file(&name) {
            merge_service_contents(services, &name, &bytes);
            continue;
        }

        let destination = staging_root.join(&name);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        if destination.exists() {
            if name.ends_with(".class") {
                let existing = fs::read(&destination)?;
                if existing != bytes {
                    warnings.push(format!(
                        "duplicate class `{}` while building fat-jar (keeping first occurrence)",
                        name
                    ));
                }
            }
            continue;
        }

        fs::write(destination, bytes)?;
    }

    Ok(())
}

fn overlay_directory_into_staging(
    source_root: &Path,
    staging_root: &Path,
    services: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), BuildError> {
    if !source_root.exists() {
        return Ok(());
    }

    overlay_directory_recursive(source_root, source_root, staging_root, services)
}

fn overlay_directory_recursive(
    base_root: &Path,
    current: &Path,
    staging_root: &Path,
    services: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), BuildError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            overlay_directory_recursive(base_root, &path, staging_root, services)?;
            continue;
        }

        let relative = path
            .strip_prefix(base_root)
            .map_err(|_| BuildError::StripPrefix(path.clone()))?;
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        if should_skip_jar_entry(&relative_str) {
            continue;
        }

        let bytes = fs::read(&path)?;
        if is_service_file(&relative_str) {
            merge_service_contents(services, &relative_str, &bytes);
            continue;
        }

        let destination = staging_root.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, bytes)?;
    }

    Ok(())
}

fn write_merged_service_files(
    staging_root: &Path,
    services: &BTreeMap<String, Vec<String>>,
) -> Result<(), BuildError> {
    for (path, lines) in services {
        let destination = staging_root.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        fs::write(destination, content)?;
    }

    Ok(())
}

pub(crate) fn merge_service_contents(
    services: &mut BTreeMap<String, Vec<String>>,
    path: &str,
    bytes: &[u8],
) {
    let bucket = services.entry(path.to_owned()).or_default();
    let mut existing = bucket.iter().cloned().collect::<HashSet<_>>();
    for line in String::from_utf8_lossy(bytes).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if existing.insert(trimmed.to_owned()) {
            bucket.push(trimmed.to_owned());
        }
    }
}

pub(crate) fn is_service_file(path: &str) -> bool {
    path.starts_with("META-INF/services/")
}

pub(crate) fn should_skip_jar_entry(path: &str) -> bool {
    if path.eq_ignore_ascii_case("META-INF/MANIFEST.MF") {
        return true;
    }

    if !path.starts_with("META-INF/") {
        return false;
    }

    path.ends_with(".SF") || path.ends_with(".RSA") || path.ends_with(".DSA")
}

pub(crate) fn is_safe_zip_path(path: &str) -> bool {
    !path.starts_with('/') && !path.split('/').any(|segment| segment == "..")
}

pub(crate) fn copy_resources(source: &Path, destination: &Path) -> Result<(), BuildError> {
    if !source.exists() {
        return Ok(());
    }

    copy_directory_contents(source, destination)?;
    Ok(())
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), BuildError> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory_contents(&source_path, &target_path)?;
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}
