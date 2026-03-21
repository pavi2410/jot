use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use quick_xml::de::from_str;
use tempfile::NamedTempFile;

use crate::format::{collect_java_files, join_classpath};
use crate::models::PmdReport;
use crate::{
    DEFAULT_PMD_RULESET, DevTools, DevToolsError, PMD_CLI_COORD, PMD_JAVA_COORD, PMD_MAIN_CLASS,
    spinner, write_path_list,
};

#[derive(Debug)]
pub struct LintReport {
    pub project: jot_config::ProjectBuildConfig,
    pub files_scanned: usize,
    pub violations: Vec<LintViolation>,
    pub processing_errors: Vec<LintProcessingError>,
}

#[derive(Debug)]
pub struct LintViolation {
    pub path: PathBuf,
    pub begin_line: usize,
    pub end_line: usize,
    pub begin_column: usize,
    pub end_column: usize,
    pub rule: String,
    pub ruleset: String,
    pub priority: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct LintProcessingError {
    pub path: PathBuf,
    pub message: String,
}

impl DevTools {
    pub fn lint(&self, project_root: &Path) -> Result<LintReport, DevToolsError> {
        let project = jot_config::load_project_build_config(project_root)?;
        let toolchain = project
            .toolchain
            .clone()
            .ok_or_else(|| DevToolsError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain)?;
        let java_files = collect_java_files(&project);
        if java_files.is_empty() {
            return Ok(LintReport {
                project,
                files_scanned: 0,
                violations: Vec::new(),
                processing_errors: Vec::new(),
            });
        }

        let resolve_progress = spinner("Resolving PMD runtime");
        let classpath = self.resolve_tool_classpath(&[PMD_CLI_COORD, PMD_JAVA_COORD])?;
        resolve_progress.finish_with_message("Resolved PMD runtime");
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

        let mut command = Command::new(installed_jdk.java_binary());
        command
            .current_dir(&project.project_root)
            .arg("-cp")
            .arg(join_classpath(&classpath)?)
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

        let lint_progress = spinner(&format!("Running PMD on {} Java files", java_files.len()));
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

        Ok(LintReport {
            project,
            files_scanned: java_files.len(),
            violations: parsed
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
                .collect(),
            processing_errors: parsed
                .errors
                .into_iter()
                .map(|error| LintProcessingError {
                    path: PathBuf::from(error.filename),
                    message: error.message,
                })
                .collect(),
        })
    }
}
