mod audit;
mod format;
mod lint;
mod models;

pub use audit::{AuditFinding, AuditReport, AuditSeverity};
pub use format::{FormatIssue, FormatReport};
pub use lint::{LintProcessingError, LintReport, LintViolation};

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use jot_resolver::{MavenCoordinate, MavenResolver};
use jot_toolchain::ToolchainManager;
use reqwest::blocking::Client;
use tempfile::NamedTempFile;

pub(crate) const DEFAULT_RESOLVE_DEPTH: usize = 8;
pub(crate) const GOOGLE_JAVA_FORMAT_COORD: &str =
    "com.google.googlejavaformat:google-java-format:1.24.0:all-deps";
pub(crate) const GOOGLE_JAVA_FORMAT_MAIN_CLASS: &str = "com.google.googlejavaformat.java.Main";
pub(crate) const GOOGLE_JAVA_FORMAT_EXPORTS: &[&str] = &[
    "jdk.compiler/com.sun.tools.javac.api=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.code=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.file=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.parser=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.tree=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.util=ALL-UNNAMED",
];
pub(crate) const PMD_CLI_COORD: &str = "net.sourceforge.pmd:pmd-cli:7.14.0";
pub(crate) const PMD_JAVA_COORD: &str = "net.sourceforge.pmd:pmd-java:7.14.0";
pub(crate) const PMD_MAIN_CLASS: &str = "net.sourceforge.pmd.cli.PmdCli";

pub(crate) const KTLINT_COORD: &str = "com.pinterest.ktlint:ktlint-cli:1.5.0:all";

pub(crate) const DETEKT_CLI_COORD: &str = "io.gitlab.arturbosch.detekt:detekt-cli:1.23.8:all";
pub(crate) const DETEKT_MAIN_CLASS: &str = "io.gitlab.arturbosch.detekt.cli.Main";

pub(crate) const DEFAULT_DETEKT_CONFIG: &str = r#"build:
  maxIssues: 0

complexity:
  active: true
  LongMethod:
    active: true
    threshold: 60
  LargeClass:
    active: true
    threshold: 600

style:
  active: true
  MagicNumber:
    active: false
  WildcardImport:
    active: true

exceptions:
  active: true
  TooGenericExceptionCaught:
    active: true
"#;

pub(crate) const DEFAULT_PMD_RULESET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset name="jot-java"
    xmlns="http://pmd.sourceforge.net/ruleset/2.0.0"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    xsi:schemaLocation="http://pmd.sourceforge.net/ruleset/2.0.0 https://pmd.github.io/schema/ruleset_2_0_0.xsd">
    <description>Default Java ruleset used by jot lint.</description>
    <rule ref="category/java/bestpractices.xml" />
    <rule ref="category/java/codestyle.xml" />
    <rule ref="category/java/errorprone.xml" />
</ruleset>
"#;

#[derive(Debug)]
pub struct DevTools {
    pub(crate) resolver: MavenResolver,
    pub(crate) toolchains: ToolchainManager,
    pub(crate) osv: Client,
}

impl DevTools {
    pub fn new(
        resolver: MavenResolver,
        toolchains: ToolchainManager,
    ) -> Result<Self, DevToolsError> {
        Ok(Self {
            resolver,
            toolchains,
            osv: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(20))
                .build()?,
        })
    }

    pub(crate) fn resolve_exact_tool_artifact(
        &self,
        coordinate: &str,
    ) -> Result<PathBuf, DevToolsError> {
        let coordinate = MavenCoordinate::parse(coordinate)?;
        let resolved = self.resolver.resolve_coordinate(&coordinate.to_string())?;
        Ok(self.resolver.cache_artifact(&resolved)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DevToolsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("resolver error: {0}")]
    Resolver(#[from] jot_resolver::ResolverError),
    #[error("config error: {0}")]
    Config(#[from] jot_config::ConfigError),
    #[error("toolchain error: {0}")]
    Toolchain(#[from] jot_toolchain::ToolchainError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to parse pmd report xml: {0}")]
    Xml(#[from] quick_xml::DeError),
    #[error("tool `{tool}` failed: {stderr}")]
    ToolFailed { tool: &'static str, stderr: String },
    #[error("project at {0} does not declare a Java toolchain")]
    MissingJavaToolchain(PathBuf),
    #[error("invalid utf-8 emitted by tool: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("invalid classpath: {0}")]
    JoinPaths(#[from] std::env::JoinPathsError),
    #[error("audit state mismatch: {0}")]
    AuditInvariant(String),
    #[error("failed to edit toml: {0}")]
    EditToml(#[from] toml_edit::TomlError),
    #[error("failed to serialize toml: {0}")]
    SerializeToml(#[from] toml::ser::Error),
}

// ── Shared utilities ────────────────────────────────────────────────────────

pub(crate) fn spinner(message: &str) -> ProgressBar {
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .expect("valid spinner template")
            .tick_strings(&["-", "\\", "|", "/"]),
    );
    progress.enable_steady_tick(std::time::Duration::from_millis(100));
    progress.set_message(message.to_owned());
    progress
}

pub(crate) fn count_bar(total: usize, message: &str) -> ProgressBar {
    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} [{bar:40.cyan/blue}] {pos}/{len}")
            .expect("valid progress bar template")
            .progress_chars("=> "),
    );
    progress.set_message(message.to_owned());
    progress
}

pub(crate) fn write_path_list(paths: &[PathBuf]) -> Result<NamedTempFile, DevToolsError> {
    let file = NamedTempFile::new()?;
    let body = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(file.path(), format!("{body}\n"))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::audit::{
        parse_cvss_score, parse_severity, rewrite_coordinate, severity_for_vulnerability,
    };
    use super::models::OsvVulnerability;
    use super::{AuditSeverity, DEFAULT_PMD_RULESET};
    use std::collections::BTreeMap;

    #[test]
    fn rewrites_matching_coordinates() {
        let mut replacements = BTreeMap::new();
        replacements.insert(
            ("org.slf4j".to_owned(), "slf4j-api".to_owned()),
            "2.0.17".to_owned(),
        );

        let updated = rewrite_coordinate("org.slf4j:slf4j-api:2.0.9", &replacements);
        assert_eq!(updated.as_deref(), Some("org.slf4j:slf4j-api:2.0.17"));
    }

    #[test]
    fn ignores_non_matching_coordinates() {
        let replacements = BTreeMap::new();
        assert!(rewrite_coordinate("org.slf4j:slf4j-api:2.0.9", &replacements).is_none());
    }

    #[test]
    fn parses_known_severity_levels() {
        assert_eq!(parse_severity("high"), AuditSeverity::High);
        assert_eq!(parse_severity("critical"), AuditSeverity::Critical);
        assert_eq!(parse_severity("medium"), AuditSeverity::Moderate);
    }

    #[test]
    fn parses_cvss_v3_vectors() {
        let score = parse_cvss_score(
            "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H",
            Some("CVSS_V3"),
        )
        .expect("cvss vector should parse");
        assert_eq!(score, 10.0);
    }

    #[test]
    fn derives_severity_from_top_level_osv_scores() {
        let vulnerability: OsvVulnerability = serde_json::from_str(
            r#"{
                "id": "CVE-2021-44228",
                "summary": "Log4Shell",
                "severity": [
                    {
                        "type": "CVSS_V3",
                        "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H"
                    }
                ]
            }"#,
        )
        .expect("valid osv payload");

        assert_eq!(
            severity_for_vulnerability(&vulnerability),
            AuditSeverity::Critical
        );
    }

    #[test]
    fn derives_severity_from_top_level_database_specific_field() {
        let vulnerability: OsvVulnerability = serde_json::from_str(
            r#"{
                "id": "GHSA-25qh-j22f-pwp8",
                "summary": "logback-core issue",
                "database_specific": {
                    "severity": "MODERATE"
                }
            }"#,
        )
        .expect("valid osv payload");

        assert_eq!(
            severity_for_vulnerability(&vulnerability),
            AuditSeverity::Moderate
        );
    }

    #[test]
    fn bundled_ruleset_references_java_categories() {
        assert!(DEFAULT_PMD_RULESET.contains("category/java/bestpractices.xml"));
        assert!(DEFAULT_PMD_RULESET.contains("category/java/errorprone.xml"));
    }
}
