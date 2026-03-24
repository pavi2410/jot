use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jot_config::{JavaFormatStyle, ProjectBuildConfig};

use super::{FormatFileResult, Formatter, JavaToolContext, describe_format_issue};
use crate::{DevToolsError, GOOGLE_JAVA_FORMAT_EXPORTS, GOOGLE_JAVA_FORMAT_MAIN_CLASS};

pub(crate) struct GoogleJavaFormat {
    classpath: Vec<PathBuf>,
    style: JavaFormatStyle,
}

impl GoogleJavaFormat {
    pub fn new(jar: PathBuf, style: JavaFormatStyle) -> Self {
        Self {
            classpath: vec![jar],
            style,
        }
    }
}

impl Formatter for GoogleJavaFormat {
    fn name(&self) -> &'static str {
        "google-java-format"
    }

    fn collect_files(&self, project: &ProjectBuildConfig) -> Vec<PathBuf> {
        project.source_files_by_ext("java")
    }

    fn format_file(
        &self,
        ctx: &JavaToolContext,
        file: &Path,
        check: bool,
    ) -> Result<FormatFileResult, DevToolsError> {
        let original = fs::read_to_string(file)?;

        let mut command = Command::new(&ctx.java_binary);
        for export in GOOGLE_JAVA_FORMAT_EXPORTS {
            command.arg("--add-exports").arg(export);
        }
        command
            .arg("-cp")
            .arg(std::env::join_paths(&self.classpath)?)
            .arg(GOOGLE_JAVA_FORMAT_MAIN_CLASS);
        if self.style == JavaFormatStyle::Aosp {
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
        let formatted = String::from_utf8(output.stdout)?;

        if formatted == original {
            return Ok(FormatFileResult {
                changed: false,
                issues: Vec::new(),
            });
        }

        let mut issues = Vec::new();
        if check {
            issues.push(describe_format_issue(file, &original, &formatted));
        } else {
            fs::write(file, &formatted)?;
        }

        Ok(FormatFileResult {
            changed: true,
            issues,
        })
    }
}
