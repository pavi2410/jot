use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jot_config::ProjectBuildConfig;

use super::{FormatContext, FormatFileResult, FormatIssue, Formatter, collect_files_with_ext};
use crate::DevToolsError;

pub(crate) struct Ktlint {
    jar: PathBuf,
}

impl Ktlint {
    pub fn new(jar: PathBuf) -> Self {
        Self { jar }
    }
}

impl Formatter for Ktlint {
    fn name(&self) -> &'static str {
        "ktlint"
    }

    fn collect_files(&self, project: &ProjectBuildConfig) -> Vec<PathBuf> {
        collect_files_with_ext(project, "kt")
    }

    fn format_file(
        &self,
        ctx: &FormatContext,
        file: &Path,
        check: bool,
    ) -> Result<FormatFileResult, DevToolsError> {
        let original = fs::read_to_string(file)?;

        let mut command = Command::new(&ctx.java_binary);
        command.arg("-jar").arg(&self.jar);
        if !check {
            command.arg("--format");
        }
        command.arg(file);

        let output = command.output()?;

        if check {
            if output.status.success() {
                return Ok(FormatFileResult {
                    changed: false,
                    issues: Vec::new(),
                });
            }

            // ktlint outputs violations as: path:line:col: message
            let stdout = String::from_utf8_lossy(&output.stdout);
            let issues = parse_ktlint_violations(&stdout, file, &original);

            Ok(FormatFileResult {
                changed: true,
                issues,
            })
        } else {
            if !output.status.success() {
                return Err(DevToolsError::ToolFailed {
                    tool: "ktlint",
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                });
            }

            let after = fs::read_to_string(file)?;
            Ok(FormatFileResult {
                changed: after != original,
                issues: Vec::new(),
            })
        }
    }
}

/// Parse ktlint stdout lines like `/path/to/File.kt:4:16: message (rule-id)`
/// into proper `FormatIssue`s with actual source lines from the file.
fn parse_ktlint_violations(stdout: &str, file: &Path, source: &str) -> Vec<FormatIssue> {
    let source_lines: Vec<&str> = source.lines().collect();
    let file_str = file.to_string_lossy();

    stdout
        .lines()
        .filter_map(|line| {
            // Strip the file path prefix to get "line:col: message"
            let rest = line
                .strip_prefix(file_str.as_ref())
                .or_else(|| {
                    // ktlint might use a different path form; try matching after last colon-digit
                    line.find(|c: char| c.is_ascii_digit())
                        .map(|pos| &line[pos..])
                })
                .unwrap_or(line);
            let rest = rest.strip_prefix(':')?;

            let mut parts = rest.splitn(3, ':');
            let line_num: usize = parts.next()?.trim().parse().ok()?;
            let col: usize = parts.next()?.trim().parse().ok()?;
            let message = parts.next().unwrap_or("").trim().to_owned();

            let actual_line = source_lines
                .get(line_num.saturating_sub(1))
                .copied()
                .unwrap_or("")
                .to_owned();

            Some(FormatIssue {
                path: file.to_path_buf(),
                line: line_num,
                column: col,
                actual_line,
                expected_line: message,
            })
        })
        .collect()
}
