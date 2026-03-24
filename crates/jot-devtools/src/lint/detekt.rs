use std::fs;
use std::path::PathBuf;
use std::process::Command;

use jot_config::ProjectBuildConfig;
use quick_xml::de::from_str;
use tempfile::NamedTempFile;

use super::{LintContext, LintResult, LintViolation, Linter};
use crate::models::CheckstyleReport;
use crate::{DEFAULT_DETEKT_CONFIG, DETEKT_CLI_COORD, DETEKT_MAIN_CLASS, DevToolsError};

pub(crate) struct Detekt {
    jar: PathBuf,
}

impl Detekt {
    pub fn new(resolver: &jot_resolver::MavenResolver) -> Result<Self, DevToolsError> {
        let resolved = resolver.resolve_coordinate(DETEKT_CLI_COORD)?;
        let jar = resolver.cache_artifact(&resolved.as_coordinate())?;
        Ok(Self { jar })
    }
}

impl Linter for Detekt {
    fn name(&self) -> &'static str {
        "detekt"
    }

    fn is_applicable(&self, project: &ProjectBuildConfig) -> bool {
        project.kotlin_toolchain.is_some() && !project.source_files_by_ext("kt").is_empty()
    }

    fn lint(
        &self,
        ctx: &LintContext,
        project: &ProjectBuildConfig,
    ) -> Result<LintResult, DevToolsError> {
        let kotlin_files = project.source_files_by_ext("kt");
        if kotlin_files.is_empty() {
            return Ok(LintResult {
                files_scanned: 0,
                violations: Vec::new(),
                processing_errors: Vec::new(),
            });
        }

        let detekt_config = NamedTempFile::new()?;
        fs::write(detekt_config.path(), DEFAULT_DETEKT_CONFIG)?;

        let detekt_report = NamedTempFile::new()?;
        let src_dirs = project
            .source_dirs
            .iter()
            .chain(project.test_source_dirs.iter())
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(",");

        let mut command = Command::new(&ctx.java_binary);
        command
            .current_dir(&project.project_root)
            .arg("-cp")
            .arg(&self.jar)
            .arg(DETEKT_MAIN_CLASS)
            .arg("--input")
            .arg(&src_dirs)
            .arg("--config")
            .arg(detekt_config.path())
            .arg("--report")
            .arg(format!("xml:{}", detekt_report.path().display()));

        let lint_progress = jot_common::spinner(&format!(
            "Running detekt on {} Kotlin files",
            kotlin_files.len()
        ));
        let output = command.output()?;
        lint_progress.finish_with_message(format!(
            "detekt completed for {} Kotlin files",
            kotlin_files.len()
        ));

        // detekt exits 0 (no issues), 1 (issues found), 2 (config error), 3 (unexpected)
        let status = output.status.code().unwrap_or(3);
        if !matches!(status, 0 | 1) {
            return Err(DevToolsError::ToolFailed {
                tool: "detekt",
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        let xml = fs::read_to_string(detekt_report.path())?;
        let parsed = if xml.trim().is_empty() {
            CheckstyleReport::default()
        } else {
            from_str::<CheckstyleReport>(&xml)?
        };

        let violations = parsed
            .files
            .into_iter()
            .flat_map(|file| {
                let name = PathBuf::from(&file.name);
                file.errors.into_iter().map(move |error| {
                    let rule = error
                        .source
                        .rsplit('.')
                        .next()
                        .unwrap_or(&error.source)
                        .to_owned();
                    let ruleset = error
                        .source
                        .rsplit('.')
                        .nth(1)
                        .unwrap_or("detekt")
                        .to_owned();
                    LintViolation {
                        path: name.clone(),
                        begin_line: error.line,
                        end_line: error.line,
                        begin_column: error.column,
                        end_column: error.column,
                        rule,
                        ruleset,
                        priority: severity_to_priority(&error.severity),
                        message: error.message,
                    }
                })
            })
            .collect();

        Ok(LintResult {
            files_scanned: kotlin_files.len(),
            violations,
            processing_errors: Vec::new(),
        })
    }
}

fn severity_to_priority(severity: &str) -> usize {
    match severity {
        "error" => 1,
        "warning" => 2,
        "info" => 3,
        _ => 2,
    }
}
