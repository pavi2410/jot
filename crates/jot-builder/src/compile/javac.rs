use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

use jot_toolchain::InstalledJdk;

use super::{
    AnnotationProcessingConfig, CompileConfig, CompileResult, SourceCompiler, java_release_flag,
};
use crate::diagnostics::format_javac_stderr;
use crate::errors::BuildError;

pub(crate) struct Javac {
    jdk: InstalledJdk,
    annotation_processing: Option<AnnotationProcessingConfig>,
}

impl Javac {
    pub fn new(
        jdk: InstalledJdk,
        annotation_processing: Option<AnnotationProcessingConfig>,
    ) -> Self {
        Self {
            jdk,
            annotation_processing,
        }
    }
}

impl SourceCompiler for Javac {
    fn name(&self) -> &'static str {
        "javac"
    }

    fn collect_sources(&self, source_dirs: &[PathBuf]) -> Result<Vec<PathBuf>, BuildError> {
        Ok(jot_common::collect_files_by_ext(source_dirs, "java"))
    }

    fn compile(
        &self,
        config: &CompileConfig,
        sources: &[PathBuf],
    ) -> Result<CompileResult, BuildError> {
        let mut command = Command::new(self.jdk.javac_binary());
        command
            .current_dir(&config.project_root)
            .arg("-d")
            .arg(&config.output_dir);

        if !config.classpath.is_empty() {
            command
                .arg("-classpath")
                .arg(std::env::join_paths(&config.classpath)?);
        }

        if let Some(release) = java_release_flag(config.jvm_target.as_deref().unwrap_or_default()) {
            command.arg("--release").arg(release);
        }

        match &self.annotation_processing {
            Some(ap) => {
                command
                    .arg("-processorpath")
                    .arg(std::env::join_paths(&ap.processor_paths)?);
                command.arg("-s").arg(&ap.generated_sources_dir);
                for (key, value) in &ap.options {
                    command.arg(format!("-A{key}={value}"));
                }
            }
            None => {
                command.arg("-proc:none");
            }
        }

        command.args(sources);
        let output = command.output()?;
        if !output.status.success() {
            let raw_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(BuildError::CommandFailed {
                tool: "javac",
                stderr: format_javac_stderr(&raw_stderr, std::io::stderr().is_terminal()),
            });
        }

        Ok(CompileResult {
            extra_classpath: vec![config.output_dir.clone()],
        })
    }
}
