mod detekt;
mod pmd;

use std::path::{Path, PathBuf};

use jot_config::ProjectBuildConfig;

use crate::{DevTools, DevToolsError};

use detekt::Detekt;
use pmd::Pmd;

// ── Public report types ─────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LintReport {
    pub project: ProjectBuildConfig,
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

// ── Linter trait ────────────────────────────────────────────────────────────

pub(crate) struct LintContext {
    pub java_binary: PathBuf,
}

pub(crate) struct LintResult {
    pub files_scanned: usize,
    pub violations: Vec<LintViolation>,
    pub processing_errors: Vec<LintProcessingError>,
}

pub(crate) trait Linter {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
    fn is_applicable(&self, project: &ProjectBuildConfig) -> bool;
    fn lint(
        &self,
        ctx: &LintContext,
        project: &ProjectBuildConfig,
    ) -> Result<LintResult, DevToolsError>;
}

// ── Orchestration ───────────────────────────────────────────────────────────

impl DevTools {
    pub fn lint(&self, project_root: &Path) -> Result<LintReport, DevToolsError> {
        let project = jot_config::load_project_build_config(project_root)?;
        let toolchain = project
            .toolchain
            .clone()
            .ok_or_else(|| DevToolsError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain)?;

        let linters: Vec<Box<dyn Linter>> = vec![
            Box::new(Pmd::new(&self.resolver)?),
            Box::new(Detekt::new(&self.resolver)?),
        ];

        let ctx = LintContext {
            java_binary: installed_jdk.java_binary(),
        };
        let mut total_scanned = 0;
        let mut all_violations = Vec::new();
        let mut all_errors = Vec::new();

        for linter in &linters {
            if !linter.is_applicable(&project) {
                continue;
            }
            let result = linter.lint(&ctx, &project)?;
            total_scanned += result.files_scanned;
            all_violations.extend(result.violations);
            all_errors.extend(result.processing_errors);
        }

        Ok(LintReport {
            project,
            files_scanned: total_scanned,
            violations: all_violations,
            processing_errors: all_errors,
        })
    }
}
