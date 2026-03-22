use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use jot_toolchain::{InstalledJdk, InstalledKotlin};

use crate::diagnostics::format_javac_stderr;
use crate::errors::BuildError;

pub(crate) struct AnnotationProcessingConfig {
    pub processor_paths: Vec<PathBuf>,
    pub options: BTreeMap<String, String>,
    pub generated_sources_dir: PathBuf,
}

pub(crate) fn compile_sources(
    installed_jdk: &InstalledJdk,
    toolchain_version: Option<&str>,
    project_root: &Path,
    classpath_paths: &[PathBuf],
    classes_dir: &Path,
    source_files: &[PathBuf],
    annotation_processing: Option<&AnnotationProcessingConfig>,
) -> Result<(), BuildError> {
    let mut command = Command::new(installed_jdk.javac_binary());
    command.current_dir(project_root).arg("-d").arg(classes_dir);

    if !classpath_paths.is_empty() {
        command
            .arg("-classpath")
            .arg(join_paths_for_classpath(classpath_paths)?);
    }

    if let Some(release) = java_release_flag(toolchain_version.unwrap_or_default()) {
        command.arg("--release").arg(release);
    }

    match annotation_processing {
        Some(config) => {
            command
                .arg("-processorpath")
                .arg(join_paths_for_classpath(&config.processor_paths)?);
            command.arg("-s").arg(&config.generated_sources_dir);
            for (key, value) in &config.options {
                command.arg(format!("-A{key}={value}"));
            }
        }
        None => {
            command.arg("-proc:none");
        }
    }

    command.args(source_files);
    let output = command.output()?;
    if !output.status.success() {
        let raw_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(BuildError::CommandFailed {
            tool: "javac",
            stderr: format_javac_stderr(&raw_stderr, std::io::stderr().is_terminal()),
        });
    }

    Ok(())
}

pub(crate) fn compile_kotlin_sources(
    installed_kotlin: &InstalledKotlin,
    jvm_target: Option<&str>,
    project_root: &Path,
    classpath_paths: &[PathBuf],
    classes_dir: &Path,
    kotlin_files: &[PathBuf],
    java_source_roots: Option<&[PathBuf]>,
) -> Result<(), BuildError> {
    let mut command = Command::new(installed_kotlin.kotlinc_binary());
    command.current_dir(project_root).arg("-d").arg(classes_dir);

    if !classpath_paths.is_empty() {
        command
            .arg("-classpath")
            .arg(join_paths_for_classpath(classpath_paths)?);
    }

    if let Some(target) = jvm_target {
        command.arg("-jvm-target").arg(target);
    }

    if let Some(roots) = java_source_roots {
        let roots_str = roots
            .iter()
            .filter(|r| r.is_dir())
            .map(|r| r.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(",");
        if !roots_str.is_empty() {
            command.arg(format!("-Xjvm-source-roots={roots_str}"));
        }
    }

    command.args(kotlin_files);
    let output = command.output()?;
    if !output.status.success() {
        let raw_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(BuildError::CommandFailed {
            tool: "kotlinc",
            stderr: raw_stderr,
        });
    }

    Ok(())
}

pub(crate) fn join_paths_for_classpath(paths: &[PathBuf]) -> Result<OsString, BuildError> {
    std::env::join_paths(paths).map_err(BuildError::JoinPaths)
}

pub(crate) fn java_release_flag(version: &str) -> Option<String> {
    let digits = version
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}
