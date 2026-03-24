mod google_java_format;
mod ktlint;

use std::path::{Path, PathBuf};

use jot_config::ProjectBuildConfig;

use crate::{DevTools, DevToolsError, GOOGLE_JAVA_FORMAT_COORD, JavaToolContext, KTLINT_COORD};

use google_java_format::GoogleJavaFormat;
use ktlint::Ktlint;

// ── Public report types ─────────────────────────────────────────────────────

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

// ── Formatter trait ─────────────────────────────────────────────────────────

pub(crate) struct FormatFileResult {
    pub changed: bool,
    pub issues: Vec<FormatIssue>,
}

pub(crate) trait Formatter {
    fn name(&self) -> &'static str;
    fn collect_files(&self, project: &ProjectBuildConfig) -> Vec<PathBuf>;
    fn format_file(
        &self,
        ctx: &JavaToolContext,
        file: &Path,
        check: bool,
    ) -> Result<FormatFileResult, DevToolsError>;
}

// ── Orchestration ───────────────────────────────────────────────────────────

impl DevTools {
    pub fn format(&self, project_root: &Path, check: bool) -> Result<FormatReport, DevToolsError> {
        let project = jot_config::load_project_build_config(project_root)?;
        let toolchain = project
            .toolchain
            .clone()
            .ok_or_else(|| DevToolsError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain)?;

        let mut formatters: Vec<Box<dyn Formatter>> = Vec::new();

        let resolve_progress = jot_common::spinner("Resolving google-java-format runtime");
        let gjf_jar = self.resolve_exact_tool_artifact(GOOGLE_JAVA_FORMAT_COORD)?;
        resolve_progress.finish_with_message("Resolved google-java-format runtime");
        formatters.push(Box::new(GoogleJavaFormat::new(
            gjf_jar,
            project.format.java_style,
        )));

        if project.kotlin_toolchain.is_some() {
            let resolve_progress = jot_common::spinner("Resolving ktlint runtime");
            let ktlint_jar = self.resolve_exact_tool_artifact(KTLINT_COORD)?;
            resolve_progress.finish_with_message("Resolved ktlint runtime");
            formatters.push(Box::new(Ktlint::new(ktlint_jar)));
        }

        let ctx = JavaToolContext {
            java_binary: installed_jdk.java_binary(),
        };
        let mut changed_files = Vec::new();
        let mut issues = Vec::new();
        let mut files_scanned = 0;

        for formatter in &formatters {
            let files = formatter.collect_files(&project);
            if files.is_empty() {
                continue;
            }
            files_scanned += files.len();

            let progress = jot_common::count_bar(
                files.len(),
                &format!(
                    "{} {} files ({})",
                    if check { "Checking" } else { "Formatting" },
                    files.len(),
                    formatter.name(),
                ),
            );

            for file in &files {
                let result = formatter.format_file(&ctx, file, check)?;
                if result.changed {
                    changed_files.push(file.clone());
                }
                issues.extend(result.issues);
                progress.inc(1);
            }

            progress.finish_with_message(format!(
                "{} {} files ({})",
                if check { "Checked" } else { "Processed" },
                files.len(),
                formatter.name(),
            ));
        }

        Ok(FormatReport {
            project,
            checked: check,
            files_scanned,
            changed_files,
            issues,
        })
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────────

pub(crate) fn describe_format_issue(file: &Path, original: &str, formatted: &str) -> FormatIssue {
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
