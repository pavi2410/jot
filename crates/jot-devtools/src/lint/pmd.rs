use std::fs;
use std::path::PathBuf;
use std::process::Command;

use jot_config::ProjectBuildConfig;
use quick_xml::de::from_str;
use tempfile::NamedTempFile;

use super::{LintContext, LintProcessingError, LintResult, LintViolation, Linter};
use crate::models::PmdReport;
use crate::{
    DEFAULT_PMD_RULESET, DevToolsError, PMD_CLI_COORD, PMD_JAVA_COORD, PMD_MAIN_CLASS,
    write_path_list,
};

pub(crate) struct Pmd {
    classpath: Vec<PathBuf>,
}

impl Pmd {
    pub fn new(resolver: &jot_resolver::MavenResolver) -> Result<Self, DevToolsError> {
        let classpath = resolver
            .resolve_artifacts(
                &[PMD_CLI_COORD.to_owned(), PMD_JAVA_COORD.to_owned()],
                crate::DEFAULT_RESOLVE_DEPTH,
            )?
            .into_iter()
            .map(|a| a.path)
            .collect::<Vec<_>>();
        Ok(Self { classpath })
    }
}

impl Linter for Pmd {
    fn name(&self) -> &'static str {
        "pmd"
    }

    fn is_applicable(&self, project: &ProjectBuildConfig) -> bool {
        !project.source_files_by_ext("java").is_empty()
    }

    fn lint(
        &self,
        ctx: &LintContext,
        project: &ProjectBuildConfig,
    ) -> Result<LintResult, DevToolsError> {
        let java_files = project.source_files_by_ext("java");
        if java_files.is_empty() {
            return Ok(LintResult {
                files_scanned: 0,
                violations: Vec::new(),
                processing_errors: Vec::new(),
            });
        }

        let file_list = write_path_list(&java_files)?;
        let report = NamedTempFile::new()?;
        let mut bundled_ruleset = None;
        let ruleset_path = if let Some(path) = project.lint.pmd_ruleset.clone() {
            path
        } else {
            let temp = NamedTempFile::new()?;
            fs::write(temp.path(), DEFAULT_PMD_RULESET)?;
            let path = temp.path().to_path_buf();
            bundled_ruleset = Some(temp);
            path
        };

        let mut classpath = self.classpath.clone();
        classpath.sort();
        classpath.dedup();

        let mut command = Command::new(&ctx.java_binary);
        command
            .current_dir(&project.project_root)
            .arg("-cp")
            .arg(std::env::join_paths(&classpath)?)
            .arg(PMD_MAIN_CLASS)
            .arg("check")
            .arg("--file-list")
            .arg(file_list.path())
            .arg("--rulesets")
            .arg(&ruleset_path)
            .arg("--format")
            .arg("xml")
            .arg("--report-file")
            .arg(report.path())
            .arg("--no-progress")
            .arg("--relativize-paths-with")
            .arg(&project.project_root);

        let lint_progress =
            jot_common::spinner(&format!("Running PMD on {} Java files", java_files.len()));
        let output = command.output()?;
        lint_progress
            .finish_with_message(format!("PMD completed for {} Java files", java_files.len()));
        drop(bundled_ruleset);

        let status = output.status.code().unwrap_or(1);
        if !matches!(status, 0 | 4) {
            return Err(DevToolsError::ToolFailed {
                tool: "pmd",
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        let xml = fs::read_to_string(report.path())?;
        let parsed = if xml.trim().is_empty() {
            PmdReport::default()
        } else {
            from_str::<PmdReport>(&xml)?
        };

        let violations = parsed
            .files
            .into_iter()
            .flat_map(|file| {
                let name = PathBuf::from(file.name);
                file.violations
                    .into_iter()
                    .map(move |violation| LintViolation {
                        path: name.clone(),
                        begin_line: violation.begin_line,
                        end_line: violation.end_line,
                        begin_column: violation.begin_column,
                        end_column: violation.end_column,
                        rule: violation.rule,
                        ruleset: violation.ruleset,
                        priority: violation.priority,
                        message: violation.message.trim().to_owned(),
                    })
            })
            .collect();

        let processing_errors = parsed
            .errors
            .into_iter()
            .map(|error| LintProcessingError {
                path: PathBuf::from(error.filename),
                message: error.message,
            })
            .collect();

        Ok(LintResult {
            files_scanned: java_files.len(),
            violations,
            processing_errors,
        })
    }
}
