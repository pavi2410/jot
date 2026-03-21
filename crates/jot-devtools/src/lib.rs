use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use fs2::FileExt;
use indicatif::{ProgressBar, ProgressStyle};
use jot_config::{
    JavaFormatStyle, ProjectBuildConfig, find_jot_toml, find_workspace_root_jot_toml,
    load_project_build_config, load_workspace_build_config, load_workspace_dependency_set,
};
use jot_resolver::{MavenCoordinate, MavenResolver, TreeEntry};
use jot_toolchain::ToolchainManager;
use quick_xml::de::from_str;
use reqwest::blocking::Client;
use serde::Deserialize;
use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, Item, value};

const DEFAULT_RESOLVE_DEPTH: usize = 8;
const GOOGLE_JAVA_FORMAT_COORD: &str =
    "com.google.googlejavaformat:google-java-format:1.24.0:all-deps";
const GOOGLE_JAVA_FORMAT_MAIN_CLASS: &str = "com.google.googlejavaformat.java.Main";
const GOOGLE_JAVA_FORMAT_EXPORTS: &[&str] = &[
    "jdk.compiler/com.sun.tools.javac.api=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.code=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.file=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.parser=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.tree=ALL-UNNAMED",
    "jdk.compiler/com.sun.tools.javac.util=ALL-UNNAMED",
];
const PMD_CLI_COORD: &str = "net.sourceforge.pmd:pmd-cli:7.14.0";
const PMD_JAVA_COORD: &str = "net.sourceforge.pmd:pmd-java:7.14.0";
const PMD_MAIN_CLASS: &str = "net.sourceforge.pmd.cli.PmdCli";

const DEFAULT_PMD_RULESET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
    resolver: MavenResolver,
    toolchains: ToolchainManager,
    osv: Client,
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

    pub fn format(&self, project_root: &Path, check: bool) -> Result<FormatReport, DevToolsError> {
        let project = load_project_build_config(project_root)?;
        let toolchain = project
            .toolchain
            .clone()
            .ok_or_else(|| DevToolsError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain)?;
        let resolve_progress = spinner("Resolving google-java-format runtime");
        let formatter_classpath = vec![self.resolve_exact_tool_artifact(GOOGLE_JAVA_FORMAT_COORD)?];
        resolve_progress.finish_with_message("Resolved google-java-format runtime");
        let java_files = collect_java_files(&project);
        let mut changed_files = Vec::new();
        let mut issues = Vec::new();
        let progress = count_bar(
            java_files.len(),
            if check {
                "Checking Java formatting"
            } else {
                "Formatting Java files"
            },
        );

        for file in &java_files {
            let original = fs::read_to_string(file)?;
            let formatted = self.run_formatter(
                &installed_jdk.java_binary(),
                &formatter_classpath,
                project.format.java_style,
                file,
            )?;
            if formatted != original {
                changed_files.push(file.clone());
                if check {
                    issues.push(describe_format_issue(file, &original, &formatted));
                }
                if !check {
                    fs::write(file, formatted)?;
                }
            }
            progress.inc(1);
        }
        progress.finish_with_message(format!(
            "{} {} Java files",
            if check { "Checked" } else { "Processed" },
            java_files.len()
        ));

        Ok(FormatReport {
            project,
            checked: check,
            files_scanned: java_files.len(),
            changed_files,
            issues,
        })
    }

    pub fn lint(&self, project_root: &Path) -> Result<LintReport, DevToolsError> {
        let project = load_project_build_config(project_root)?;
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

    pub fn audit(&self, start: &Path, fix: bool) -> Result<AuditReport, DevToolsError> {
        let resolve_progress = spinner("Resolving dependency graph for audit");
        let context = AuditContext::load(start, &self.resolver, Some(&resolve_progress))?;
        resolve_progress.finish_with_message(format!(
            "Resolved dependency graph for {} packages",
            context.packages.len()
        ));
        let mut vulnerability_ids = HashSet::new();
        let mut package_ids = Vec::new();
        for package in context.packages.values() {
            package_ids.push(package.coordinate.clone());
        }

        let batch_progress = spinner(&format!("Querying OSV for {} packages", package_ids.len()));
        let response: OsvBatchResponse = self
            .osv
            .post("https://api.osv.dev/v1/querybatch")
            .json(&OsvBatchRequest {
                queries: package_ids
                    .iter()
                    .map(|coordinate| OsvQuery {
                        version: coordinate.version.clone().unwrap_or_default(),
                        package: OsvPackage {
                            ecosystem: "Maven".to_owned(),
                            name: format!("{}:{}", coordinate.group, coordinate.artifact),
                        },
                    })
                    .collect(),
            })
            .send()?
            .error_for_status()?
            .json()?;
        batch_progress.finish_with_message(format!(
            "Received OSV batch results for {} packages",
            package_ids.len()
        ));

        let mut package_to_vulns = BTreeMap::<String, Vec<String>>::new();
        for (index, result) in response.results.into_iter().enumerate() {
            let package_key = package_ids[index].to_string();
            let ids = result
                .vulns
                .into_iter()
                .map(|vuln| vuln.id)
                .collect::<Vec<_>>();
            vulnerability_ids.extend(ids.iter().cloned());
            if !ids.is_empty() {
                package_to_vulns.insert(package_key, ids);
            }
        }

        let mut vuln_details = HashMap::<String, OsvVulnerability>::new();
        let detail_count = vulnerability_ids.len();
        let detail_progress = count_bar(detail_count, "Fetching vulnerability details");
        for vuln_id in vulnerability_ids {
            let detail = self
                .osv
                .get(format!("https://api.osv.dev/v1/vulns/{vuln_id}"))
                .send()?
                .error_for_status()?
                .json::<OsvVulnerability>()?;
            vuln_details.insert(vuln_id, detail);
            detail_progress.inc(1);
        }
        detail_progress
            .finish_with_message(format!("Fetched {} vulnerability records", detail_count));

        let mut findings = Vec::new();
        for (package_key, vuln_ids) in package_to_vulns {
            let package = context
                .packages
                .get(&package_key)
                .ok_or_else(|| DevToolsError::AuditInvariant(package_key.clone()))?;
            for vuln_id in vuln_ids {
                let detail = vuln_details
                    .get(&vuln_id)
                    .ok_or_else(|| DevToolsError::AuditInvariant(vuln_id.clone()))?;
                findings.push(AuditFinding {
                    severity: severity_for_vulnerability(detail),
                    vuln_id: detail.id.clone(),
                    package: package.coordinate.clone(),
                    fixed_version: fixed_version_for(detail, &package.coordinate),
                    summary: detail
                        .summary
                        .clone()
                        .unwrap_or_else(|| "No summary provided".to_owned()),
                    members: package.members.iter().cloned().collect(),
                    chains: package.chains.clone(),
                });
            }
        }

        findings.sort_by(|left, right| {
            right
                .severity
                .cmp(&left.severity)
                .then_with(|| left.package.to_string().cmp(&right.package.to_string()))
                .then_with(|| left.vuln_id.cmp(&right.vuln_id))
        });

        let fixed_dependencies = if fix {
            let fix_progress = spinner("Applying vulnerability fixes");
            let fixed = apply_audit_fixes(start, &context, &findings)?;
            fix_progress
                .finish_with_message(format!("Updated {} direct dependency declarations", fixed));
            fixed
        } else {
            0
        };

        Ok(AuditReport {
            workspace_root: context.root_dir,
            packages_scanned: context.packages.len(),
            findings,
            fixed_dependencies,
        })
    }

    fn resolve_tool_classpath(&self, coordinates: &[&str]) -> Result<Vec<PathBuf>, DevToolsError> {
        let mut classpath = self
            .resolver
            .resolve_artifacts(
                &coordinates
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect::<Vec<_>>(),
                DEFAULT_RESOLVE_DEPTH,
            )?
            .into_iter()
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        classpath.sort();
        classpath.dedup();
        Ok(classpath)
    }

    fn resolve_exact_tool_artifact(&self, coordinate: &str) -> Result<PathBuf, DevToolsError> {
        let coordinate = MavenCoordinate::parse(coordinate)?;
        let resolved = self.resolver.resolve_coordinate(&coordinate.to_string())?;
        Ok(self.resolver.cache_artifact(&resolved)?)
    }

    fn run_formatter(
        &self,
        java_binary: &Path,
        classpath: &[PathBuf],
        style: JavaFormatStyle,
        file: &Path,
    ) -> Result<String, DevToolsError> {
        let mut command = Command::new(java_binary);
        for export in GOOGLE_JAVA_FORMAT_EXPORTS {
            command.arg("--add-exports").arg(export);
        }
        command
            .arg("-cp")
            .arg(join_classpath(classpath)?)
            .arg(GOOGLE_JAVA_FORMAT_MAIN_CLASS);
        if style == JavaFormatStyle::Aosp {
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
        Ok(String::from_utf8(output.stdout)?)
    }
}

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

#[derive(Debug)]
pub struct AuditReport {
    pub workspace_root: PathBuf,
    pub packages_scanned: usize,
    pub findings: Vec<AuditFinding>,
    pub fixed_dependencies: usize,
}

#[derive(Debug, Clone)]
pub struct AuditFinding {
    pub severity: AuditSeverity,
    pub vuln_id: String,
    pub package: MavenCoordinate,
    pub fixed_version: Option<String>,
    pub summary: String,
    pub members: Vec<String>,
    pub chains: Vec<Vec<MavenCoordinate>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditSeverity {
    Unknown,
    Low,
    Moderate,
    High,
    Critical,
}

impl AuditSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Low => "LOW",
            Self::Moderate => "MODERATE",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }

    pub fn is_ci_failure(self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }

    fn from_cvss_score(score: f64) -> Self {
        if score >= 9.0 {
            Self::Critical
        } else if score >= 7.0 {
            Self::High
        } else if score >= 4.0 {
            Self::Moderate
        } else if score > 0.0 {
            Self::Low
        } else {
            Self::Unknown
        }
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

#[derive(Debug)]
struct AuditContext {
    root_dir: PathBuf,
    packages: BTreeMap<String, AuditPackageContext>,
}

#[derive(Debug)]
struct AuditPackageContext {
    coordinate: MavenCoordinate,
    members: BTreeSet<String>,
    chains: Vec<Vec<MavenCoordinate>>,
}

impl AuditContext {
    fn load(
        start: &Path,
        resolver: &MavenResolver,
        progress: Option<&ProgressBar>,
    ) -> Result<Self, DevToolsError> {
        if let Some(workspace) = load_workspace_dependency_set(start)? {
            let mut root_to_members = BTreeMap::<String, BTreeSet<String>>::new();
            for member in &workspace.members {
                for dependency in &member.external_dependencies {
                    root_to_members
                        .entry(dependency.clone())
                        .or_default()
                        .insert(member.module_name.clone());
                }
            }
            let packages = collect_audit_packages(
                resolver,
                &workspace.external_dependencies,
                &root_to_members,
                progress,
            )?;
            return Ok(Self {
                root_dir: workspace.root_dir,
                packages,
            });
        }

        let config_path = find_jot_toml(start)?
            .ok_or_else(|| DevToolsError::AuditInvariant("could not find jot.toml".to_owned()))?;
        let project = load_project_build_config(start)?;
        let project_name = project
            .module_name
            .clone()
            .unwrap_or_else(|| project.name.clone());
        let mut root_to_members = BTreeMap::<String, BTreeSet<String>>::new();
        for dependency in &project.dependencies {
            root_to_members
                .entry(dependency.clone())
                .or_default()
                .insert(project_name.clone());
        }
        Ok(Self {
            root_dir: config_path.parent().unwrap_or(start).to_path_buf(),
            packages: collect_audit_packages(
                resolver,
                &project.dependencies,
                &root_to_members,
                progress,
            )?,
        })
    }
}

fn collect_audit_packages(
    resolver: &MavenResolver,
    roots: &[String],
    root_to_members: &BTreeMap<String, BTreeSet<String>>,
    progress: Option<&ProgressBar>,
) -> Result<BTreeMap<String, AuditPackageContext>, DevToolsError> {
    let mut packages = BTreeMap::<String, AuditPackageContext>::new();

    for root in roots {
        if let Some(progress) = progress {
            progress.set_message(format!("Resolving dependency graph for audit: {root}"));
        }
        let entries = resolver.resolve_dependency_tree(root, DEFAULT_RESOLVE_DEPTH)?;
        let root_members = root_to_members.get(root).cloned().unwrap_or_default();
        let mut chain = Vec::<MavenCoordinate>::new();
        for entry in entries {
            push_tree_entry(&mut chain, &entry);
            let key = entry.coordinate.to_string();
            let package = packages.entry(key).or_insert_with(|| AuditPackageContext {
                coordinate: entry.coordinate.clone(),
                members: BTreeSet::new(),
                chains: Vec::new(),
            });
            package.members.extend(root_members.iter().cloned());
            if !package.chains.iter().any(|existing| existing == &chain) {
                package.chains.push(chain.clone());
            }
        }
    }

    Ok(packages)
}

fn push_tree_entry(chain: &mut Vec<MavenCoordinate>, entry: &TreeEntry) {
    chain.truncate(entry.depth);
    chain.push(entry.coordinate.clone());
}

fn collect_java_files(project: &ProjectBuildConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for source_dir in project
        .source_dirs
        .iter()
        .chain(project.test_source_dirs.iter())
    {
        visit_java_files(source_dir, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

fn visit_java_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_java_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("java") {
            files.push(path);
        }
    }
}

fn join_classpath(paths: &[PathBuf]) -> Result<OsString, DevToolsError> {
    Ok(std::env::join_paths(paths.iter())?)
}

fn write_path_list(paths: &[PathBuf]) -> Result<NamedTempFile, DevToolsError> {
    let file = NamedTempFile::new()?;
    let body = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(file.path(), format!("{body}\n"))?;
    Ok(file)
}

fn spinner(message: &str) -> ProgressBar {
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

fn count_bar(total: usize, message: &str) -> ProgressBar {
    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} [{bar:40.cyan/blue}] {pos}/{len}")
            .expect("valid progress bar template")
            .progress_chars("=> "),
    );
    progress.set_message(message.to_owned());
    progress
}

fn describe_format_issue(file: &Path, original: &str, formatted: &str) -> FormatIssue {
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

fn apply_audit_fixes(
    start: &Path,
    _context: &AuditContext,
    findings: &[AuditFinding],
) -> Result<usize, DevToolsError> {
    let mut fixed = 0;
    let mut by_package = BTreeMap::<(String, String), String>::new();
    for finding in findings {
        let Some(version) = &finding.fixed_version else {
            continue;
        };
        by_package
            .entry((
                finding.package.group.clone(),
                finding.package.artifact.clone(),
            ))
            .or_insert_with(|| version.clone());
    }

    if by_package.is_empty() {
        return Ok(0);
    }

    if let Some(workspace) = load_workspace_build_config(start)? {
        for member in &workspace.members {
            fixed += update_project_dependencies(&member.project.config_path, &by_package)?;
        }
        let inputs = load_workspace_dependency_set(&workspace.root_dir)?
            .map(|set| set.external_dependencies)
            .unwrap_or_default();
        let lockfile = context_root_lockfile_path(&workspace.root_dir);
        rewrite_lockfile(start, &workspace.root_dir, &inputs, &lockfile)?;
        return Ok(fixed);
    }

    let config_path = find_jot_toml(start)?
        .ok_or_else(|| DevToolsError::AuditInvariant("could not find jot.toml".to_owned()))?;
    fixed += update_project_dependencies(&config_path, &by_package)?;
    let project = load_project_build_config(start)?;
    let lockfile = project.project_root.join("jot.lock");
    rewrite_lockfile(
        start,
        &project.project_root,
        &project.dependencies,
        &lockfile,
    )?;
    Ok(fixed)
}

fn rewrite_lockfile(
    start: &Path,
    _root: &Path,
    inputs: &[String],
    output_path: &Path,
) -> Result<(), DevToolsError> {
    let config_path =
        find_workspace_root_jot_toml(start)?.or_else(|| find_jot_toml(start).ok().flatten());
    let Some(_root_config) = config_path else {
        return Ok(());
    };
    let paths = jot_cache::JotPaths::new()
        .map_err(|error| DevToolsError::AuditInvariant(error.to_string()))?;
    let resolver = MavenResolver::new(paths)?;
    let lockfile = resolver.resolve_lockfile(inputs, DEFAULT_RESOLVE_DEPTH)?;
    write_locked_file(
        output_path,
        toml::to_string_pretty(&lockfile)?.as_bytes(),
        &jot_cache::JotPaths::new()
            .map_err(|error| DevToolsError::AuditInvariant(error.to_string()))?,
    )?;
    Ok(())
}

fn context_root_lockfile_path(root: &Path) -> PathBuf {
    root.join("jot.lock")
}

fn update_project_dependencies(
    config_path: &Path,
    replacements: &BTreeMap<(String, String), String>,
) -> Result<usize, DevToolsError> {
    let content = fs::read_to_string(config_path)?;
    let mut document = content.parse::<DocumentMut>()?;
    let mut changes = 0;
    for section in ["dependencies", "test-dependencies"] {
        let Some(table) = document.get_mut(section).and_then(Item::as_table_like_mut) else {
            continue;
        };
        let keys = table
            .iter()
            .map(|(key, _)| key.to_owned())
            .collect::<Vec<_>>();
        for key in keys {
            let Some(item) = table.get_mut(&key) else {
                continue;
            };
            changes += update_dependency_item(item, replacements);
        }
    }

    if changes > 0 {
        fs::write(config_path, document.to_string())?;
    }
    Ok(changes)
}

fn update_dependency_item(
    item: &mut Item,
    replacements: &BTreeMap<(String, String), String>,
) -> usize {
    if let Some(coords) = item.as_str() {
        if let Some(updated) = rewrite_coordinate(coords, replacements) {
            *item = value(updated);
            return 1;
        }
        return 0;
    }

    let Some(inline) = item.as_inline_table_mut() else {
        return 0;
    };
    let Some(coords) = inline.get("coords").and_then(|value| value.as_str()) else {
        return 0;
    };
    let Some(updated) = rewrite_coordinate(coords, replacements) else {
        return 0;
    };
    inline.insert("coords", toml_edit::Value::from(updated));
    1
}

fn write_locked_file(
    output_path: &Path,
    content: &[u8],
    paths: &jot_cache::JotPaths,
) -> Result<(), DevToolsError> {
    let lock_path = paths.locks_dir().join(format!(
        "file-{}.lock",
        sanitize_for_filename(&output_path.to_string_lossy())
    ));
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    lock_file
        .lock_exclusive()
        .map_err(|error| DevToolsError::AuditInvariant(error.to_string()))?;

    let parent = output_path.parent().ok_or_else(|| {
        DevToolsError::AuditInvariant(format!(
            "path {} has no parent directory",
            output_path.display()
        ))
    })?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content)?;
    temp_file.flush()?;
    if output_path.exists() {
        fs::remove_file(output_path)?;
    }
    temp_file
        .persist(output_path)
        .map_err(|error| DevToolsError::Io(error.error))?;

    let _ = lock_file.unlock();
    Ok(())
}

fn sanitize_for_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

fn rewrite_coordinate(
    coords: &str,
    replacements: &BTreeMap<(String, String), String>,
) -> Option<String> {
    let mut parts = coords.split(':').collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    let key = (parts[0].to_owned(), parts[1].to_owned());
    let replacement = replacements.get(&key)?;
    parts[2] = replacement;
    Some(parts.join(":"))
}

fn severity_for_vulnerability(vuln: &OsvVulnerability) -> AuditSeverity {
    let mut severity = AuditSeverity::Unknown;

    for candidate in &vuln.severity {
        severity = severity.max(parse_osv_severity(candidate));
    }

    if let Some(candidate) = vuln
        .ecosystem_specific
        .as_ref()
        .and_then(|value| value.severity.as_deref())
    {
        severity = severity.max(parse_severity(candidate));
    }

    if let Some(candidate) = vuln
        .database_specific
        .as_ref()
        .and_then(|value| value.severity.as_deref())
    {
        severity = severity.max(parse_severity(candidate));
    }

    for affected in &vuln.affected {
        for candidate in &affected.severity {
            severity = severity.max(parse_osv_severity(candidate));
        }

        if let Some(candidate) = affected
            .ecosystem_specific
            .as_ref()
            .and_then(|value| value.severity.as_deref())
        {
            severity = severity.max(parse_severity(candidate));
        }

        if let Some(candidate) = affected
            .database_specific
            .as_ref()
            .and_then(|value| value.severity.as_deref())
        {
            severity = severity.max(parse_severity(candidate));
        }
    }

    severity
}

fn parse_severity(input: &str) -> AuditSeverity {
    match input.trim().to_ascii_uppercase().as_str() {
        "LOW" => AuditSeverity::Low,
        "MODERATE" | "MEDIUM" | "MEDIUM_LOW" | "MEDIUM_HIGH" => AuditSeverity::Moderate,
        "HIGH" => AuditSeverity::High,
        "CRITICAL" => AuditSeverity::Critical,
        _ => AuditSeverity::Unknown,
    }
}

fn parse_osv_severity(severity: &OsvSeverity) -> AuditSeverity {
    if let Some(score) = parse_cvss_score(&severity.score, severity.kind.as_deref()) {
        return AuditSeverity::from_cvss_score(score);
    }

    parse_severity(&severity.score)
}

fn parse_cvss_score(value: &str, kind: Option<&str>) -> Option<f64> {
    let trimmed = value.trim();
    if let Ok(score) = trimmed.parse::<f64>() {
        return Some(score);
    }

    let is_v3 = kind
        .map(|candidate| candidate.eq_ignore_ascii_case("CVSS_V3"))
        .unwrap_or_else(|| trimmed.starts_with("CVSS:3."));

    if is_v3 {
        return parse_cvss_v3_vector(trimmed);
    }

    None
}

fn parse_cvss_v3_vector(vector: &str) -> Option<f64> {
    let mut attack_vector = None;
    let mut attack_complexity = None;
    let mut privileges_required = None;
    let mut user_interaction = None;
    let mut scope = None;
    let mut confidentiality = None;
    let mut integrity = None;
    let mut availability = None;

    for component in vector.split('/') {
        if component.starts_with("CVSS:") {
            continue;
        }
        let (metric, value) = component.split_once(':')?;
        match metric {
            "AV" => attack_vector = Some(cvss_attack_vector(value)?),
            "AC" => attack_complexity = Some(cvss_attack_complexity(value)?),
            "PR" => privileges_required = Some(value.to_owned()),
            "UI" => user_interaction = Some(cvss_user_interaction(value)?),
            "S" => scope = Some(value.to_owned()),
            "C" => confidentiality = Some(cvss_impact(value)?),
            "I" => integrity = Some(cvss_impact(value)?),
            "A" => availability = Some(cvss_impact(value)?),
            _ => {}
        }
    }

    let scope = scope?;
    let attack_vector = attack_vector?;
    let attack_complexity = attack_complexity?;
    let user_interaction = user_interaction?;
    let confidentiality = confidentiality?;
    let integrity = integrity?;
    let availability = availability?;
    let privileges_required = cvss_privileges_required(privileges_required?.as_str(), &scope)?;

    let impact_sub_score = 1.0 - (1.0 - confidentiality) * (1.0 - integrity) * (1.0 - availability);
    let impact = if scope == "U" {
        6.42 * impact_sub_score
    } else {
        7.52 * (impact_sub_score - 0.029) - 3.25 * (impact_sub_score - 0.02).powf(15.0)
    };

    if impact <= 0.0 {
        return Some(0.0);
    }

    let exploitability =
        8.22 * attack_vector * attack_complexity * privileges_required * user_interaction;
    let score = if scope == "U" {
        (impact + exploitability).min(10.0)
    } else {
        (1.08 * (impact + exploitability)).min(10.0)
    };

    Some(round_up_cvss(score))
}

fn cvss_attack_vector(value: &str) -> Option<f64> {
    match value {
        "N" => Some(0.85),
        "A" => Some(0.62),
        "L" => Some(0.55),
        "P" => Some(0.20),
        _ => None,
    }
}

fn cvss_attack_complexity(value: &str) -> Option<f64> {
    match value {
        "L" => Some(0.77),
        "H" => Some(0.44),
        _ => None,
    }
}

fn cvss_privileges_required(value: &str, scope: &str) -> Option<f64> {
    match (value, scope) {
        ("N", _) => Some(0.85),
        ("L", "U") => Some(0.62),
        ("L", "C") => Some(0.68),
        ("H", "U") => Some(0.27),
        ("H", "C") => Some(0.50),
        _ => None,
    }
}

fn cvss_user_interaction(value: &str) -> Option<f64> {
    match value {
        "N" => Some(0.85),
        "R" => Some(0.62),
        _ => None,
    }
}

fn cvss_impact(value: &str) -> Option<f64> {
    match value {
        "H" => Some(0.56),
        "L" => Some(0.22),
        "N" => Some(0.0),
        _ => None,
    }
}

fn round_up_cvss(score: f64) -> f64 {
    ((score * 10.0) - 1e-10).ceil() / 10.0
}

fn fixed_version_for(vuln: &OsvVulnerability, package: &MavenCoordinate) -> Option<String> {
    let package_name = format!("{}:{}", package.group, package.artifact);
    let mut versions = vuln
        .affected
        .iter()
        .filter(|affected| {
            affected
                .package
                .as_ref()
                .is_some_and(|candidate| candidate.name == package_name)
        })
        .flat_map(|affected| affected.ranges.iter())
        .flat_map(|range| range.events.iter())
        .filter_map(|event| event.fixed.clone())
        .collect::<Vec<_>>();
    versions.sort();
    versions.dedup();
    versions.into_iter().next()
}

#[derive(Debug, Default, Deserialize)]
struct PmdReport {
    #[serde(rename = "file", default)]
    files: Vec<PmdFile>,
    #[serde(rename = "error", default)]
    errors: Vec<PmdError>,
}

#[derive(Debug, Deserialize)]
struct PmdFile {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "violation", default)]
    violations: Vec<PmdViolation>,
}

#[derive(Debug, Deserialize)]
struct PmdViolation {
    #[serde(rename = "@beginline")]
    begin_line: usize,
    #[serde(rename = "@endline")]
    end_line: usize,
    #[serde(rename = "@begincolumn")]
    begin_column: usize,
    #[serde(rename = "@endcolumn")]
    end_column: usize,
    #[serde(rename = "@rule")]
    rule: String,
    #[serde(rename = "@ruleset")]
    ruleset: String,
    #[serde(rename = "@priority")]
    priority: usize,
    #[serde(rename = "$text")]
    message: String,
}

#[derive(Debug, Deserialize)]
struct PmdError {
    #[serde(rename = "@filename")]
    filename: String,
    #[serde(rename = "@msg")]
    message: String,
}

#[derive(Debug, serde::Serialize)]
struct OsvBatchRequest {
    queries: Vec<OsvQuery>,
}

#[derive(Debug, serde::Serialize)]
struct OsvQuery {
    version: String,
    package: OsvPackage,
}

#[derive(Debug, serde::Serialize)]
struct OsvPackage {
    ecosystem: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct OsvBatchResponse {
    results: Vec<OsvBatchResult>,
}

#[derive(Debug, Deserialize)]
struct OsvBatchResult {
    #[serde(default)]
    vulns: Vec<OsvBatchVuln>,
}

#[derive(Debug, Deserialize)]
struct OsvBatchVuln {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OsvVulnerability {
    id: String,
    summary: Option<String>,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
    ecosystem_specific: Option<OsvSeverityHolder>,
    database_specific: Option<OsvSeverityHolder>,
    #[serde(default)]
    affected: Vec<OsvAffected>,
}

#[derive(Debug, Deserialize)]
struct OsvAffected {
    package: Option<OsvAffectedPackage>,
    #[serde(default)]
    ranges: Vec<OsvAffectedRange>,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
    ecosystem_specific: Option<OsvSeverityHolder>,
    database_specific: Option<OsvSeverityHolder>,
}

#[derive(Debug, Deserialize)]
struct OsvSeverity {
    #[serde(rename = "type")]
    kind: Option<String>,
    score: String,
}

#[derive(Debug, Deserialize)]
struct OsvAffectedPackage {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OsvAffectedRange {
    #[serde(default)]
    events: Vec<OsvRangeEvent>,
}

#[derive(Debug, Deserialize)]
struct OsvRangeEvent {
    fixed: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OsvSeverityHolder {
    severity: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        AuditSeverity, DEFAULT_PMD_RULESET, OsvVulnerability, parse_cvss_score, parse_severity,
        rewrite_coordinate, severity_for_vulnerability,
    };
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
