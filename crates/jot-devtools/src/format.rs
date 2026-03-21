use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jot_config::{JavaFormatStyle, ProjectBuildConfig};

use crate::{
    DevTools, DevToolsError, GOOGLE_JAVA_FORMAT_COORD, GOOGLE_JAVA_FORMAT_EXPORTS,
    GOOGLE_JAVA_FORMAT_MAIN_CLASS, count_bar, spinner,
};

#[derive(Debug)]
pub struct FormatReport {
    pub project: ProjectBuildConfig,
    pub checked: bool,
    pub files_scanned: usize,
    pub changed_files: Vec<PathBuf>,
    pub issues: Vec<FormatIssue>,
}

#[derive(Debug, Clone)]
pub struct FormatIssue {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub actual_line: String,
    pub expected_line: String,
}

impl DevTools {
    pub fn format(&self, project_root: &Path, check: bool) -> Result<FormatReport, DevToolsError> {
        let project = jot_config::load_project_build_config(project_root)?;
        let toolchain = project
            .toolchain
            .clone()
            .ok_or_else(|| DevToolsError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain)?;
        let resolve_progress = spinner("Resolving google-java-format runtime");
        let formatter_classpath = vec![self.resolve_exact_tool_artifact(GOOGLE_JAVA_FORMAT_COORD)?];
        resolve_progress.finish_with_message("Resolved google-java-format runtime");
        let java_files = collect_java_files(&project);
        let mut changed_files = Vec::new();
        let mut issues = Vec::new();
        let progress = count_bar(
            java_files.len(),
            if check {
                "Checking Java formatting"
            } else {
                "Formatting Java files"
            },
        );

        for file in &java_files {
            let original = fs::read_to_string(file)?;
            let formatted = self.run_formatter(
                &installed_jdk.java_binary(),
                &formatter_classpath,
                project.format.java_style,
                file,
            )?;
            if formatted != original {
                changed_files.push(file.clone());
                if check {
                    issues.push(describe_format_issue(file, &original, &formatted));
                }
                if !check {
                    fs::write(file, formatted)?;
                }
            }
            progress.inc(1);
        }
        progress.finish_with_message(format!(
            "{} {} Java files",
            if check { "Checked" } else { "Processed" },
            java_files.len()
        ));

        Ok(FormatReport {
            project,
            checked: check,
            files_scanned: java_files.len(),
            changed_files,
            issues,
        })
    }

    pub(crate) fn run_formatter(
        &self,
        java_binary: &Path,
        classpath: &[PathBuf],
        style: JavaFormatStyle,
        file: &Path,
    ) -> Result<String, DevToolsError> {
        let mut command = Command::new(java_binary);
        for export in GOOGLE_JAVA_FORMAT_EXPORTS {
            command.arg("--add-exports").arg(export);
        }
        command
            .arg("-cp")
            .arg(join_classpath(classpath)?)
            .arg(GOOGLE_JAVA_FORMAT_MAIN_CLASS);
        if style == JavaFormatStyle::Aosp {
            command.arg("--aosp");
        }
        command.arg(file);

        let output = command.output()?;
        if !output.status.success() {
            return Err(DevToolsError::ToolFailed {
                tool: "google-java-format",
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8(output.stdout)?)
    }
}

pub(crate) fn collect_java_files(project: &ProjectBuildConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for source_dir in project
        .source_dirs
        .iter()
        .chain(project.test_source_dirs.iter())
    {
        visit_java_files(source_dir, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

fn visit_java_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_java_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("java") {
            files.push(path);
        }
    }
}

pub(crate) fn join_classpath(paths: &[PathBuf]) -> Result<OsString, DevToolsError> {
    Ok(std::env::join_paths(paths.iter())?)
}

fn describe_format_issue(file: &Path, original: &str, formatted: &str) -> FormatIssue {
    let original_lines = original.lines().collect::<Vec<_>>();
    let formatted_lines = formatted.lines().collect::<Vec<_>>();
    let max_lines = original_lines.len().max(formatted_lines.len());

    for index in 0..max_lines {
        let actual = original_lines.get(index).copied().unwrap_or("");
        let expected = formatted_lines.get(index).copied().unwrap_or("");
        if actual != expected {
            return FormatIssue {
                path: file.to_path_buf(),
                line: index + 1,
                column: first_differing_column(actual, expected),
                actual_line: if actual.is_empty() {
                    expected.to_owned()
                } else {
                    actual.to_owned()
                },
                expected_line: expected.to_owned(),
            };
        }
    }

    FormatIssue {
        path: file.to_path_buf(),
        line: 1,
        column: 1,
        actual_line: original_lines.first().copied().unwrap_or("").to_owned(),
        expected_line: formatted_lines.first().copied().unwrap_or("").to_owned(),
    }
}

fn first_differing_column(left: &str, right: &str) -> usize {
    let mut left_chars = left.chars();
    let mut right_chars = right.chars();
    let mut column = 1;

    loop {
        match (left_chars.next(), right_chars.next()) {
            (Some(left_char), Some(right_char)) if left_char == right_char => {
                column += 1;
            }
            (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => return column,
            (None, None) => return 1,
        }
    }
}
