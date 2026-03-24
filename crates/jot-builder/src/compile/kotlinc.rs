use std::path::PathBuf;
use std::process::Command;

use jot_toolchain::InstalledKotlin;

use super::{CompileConfig, CompileResult, SourceCompiler};
use crate::errors::BuildError;

pub(crate) struct Kotlinc {
    kotlin: InstalledKotlin,
    java_source_roots: Option<Vec<PathBuf>>,
}

impl Kotlinc {
    pub fn new(kotlin: InstalledKotlin, java_source_roots: Option<Vec<PathBuf>>) -> Self {
        Self {
            kotlin,
            java_source_roots,
        }
    }
}

impl SourceCompiler for Kotlinc {
    fn name(&self) -> &'static str {
        "kotlinc"
    }

    fn collect_sources(&self, source_dirs: &[PathBuf]) -> Result<Vec<PathBuf>, BuildError> {
        Ok(jot_common::collect_files_by_ext(source_dirs, "kt"))
    }

    fn compile(
        &self,
        config: &CompileConfig,
        sources: &[PathBuf],
    ) -> Result<CompileResult, BuildError> {
        let mut command = Command::new(self.kotlin.kotlinc_binary());
        command
            .current_dir(&config.project_root)
            .arg("-d")
            .arg(&config.output_dir);

        if !config.classpath.is_empty() {
            command
                .arg("-classpath")
                .arg(std::env::join_paths(&config.classpath)?);
        }

        if let Some(ref target) = config.jvm_target {
            command.arg("-jvm-target").arg(target);
        }

        if let Some(ref roots) = self.java_source_roots {
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

        command.args(sources);
        let output = command.output()?;
        if !output.status.success() {
            let raw_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(BuildError::CommandFailed {
                tool: "kotlinc",
                stderr: raw_stderr,
            });
        }

        Ok(CompileResult {
            extra_classpath: vec![config.output_dir.clone()],
        })
    }
}
