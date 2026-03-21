use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jot_config::{
    find_jot_toml, find_workspace_root_jot_toml, load_project_build_config,
    load_workspace_build_config, load_workspace_dependency_set, JavaFormatStyle,
    ProjectBuildConfig,
};
use jot_resolver::{MavenCoordinate, MavenResolver, TreeEntry};
use jot_toolchain::ToolchainManager;
use quick_xml::de::from_str;
use reqwest::blocking::Client;
use serde::Deserialize;
use tempfile::NamedTempFile;
use toml_edit::{value, DocumentMut, Item};

const DEFAULT_RESOLVE_DEPTH: usize = 8;
const GOOGLE_JAVA_FORMAT_COORD: &str = "com.google.googlejavaformat:google-java-format:1.24.0";
const GOOGLE_JAVA_FORMAT_MAIN_CLASS: &str = "com.google.googlejavaformat.java.Main";
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
    pub fn new(resolver: MavenResolver, toolchains: ToolchainManager) -> Result<Self, DevToolsError> {
        Ok(Self {
            resolver,
            toolchains,
            osv: Client::builder().build()?,
        })
    }

    pub fn format(&self, project_root: &Path, check: bool) -> Result<FormatReport, DevToolsError> {
        let project = load_project_build_config(project_root)?;
        let toolchain = project
            .toolchain
            .clone()
            .ok_or_else(|| DevToolsError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain)?;
        let formatter_classpath = self.resolve_tool_classpath(&[GOOGLE_JAVA_FORMAT_COORD])?;
        let java_files = collect_java_files(&project);
        let mut changed_files = Vec::new();

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
                if !check {
                    fs::write(file, formatted)?;
                }
            }
        }

        Ok(FormatReport {
            project,
            checked: check,
            files_scanned: java_files.len(),
            changed_files,
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

        let classpath = self.resolve_tool_classpath(&[PMD_CLI_COORD, PMD_JAVA_COORD])?;
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

        let output = command.output()?;
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
                    file.violations.into_iter().map(move |violation| LintViolation {
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
        let context = AuditContext::load(start, &self.resolver)?;
        let mut vulnerability_ids = HashSet::new();
        let mut package_ids = Vec::new();
        for package in context.packages.values() {
            package_ids.push(package.coordinate.clone());
        }

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
        for vuln_id in vulnerability_ids {
            let detail = self
                .osv
                .get(format!("https://api.osv.dev/v1/vulns/{vuln_id}"))
                .send()?
                .error_for_status()?
                .json::<OsvVulnerability>()?;
            vuln_details.insert(vuln_id, detail);
        }

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
                    summary: detail.summary.clone().unwrap_or_else(|| "No summary provided".to_owned()),
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
            apply_audit_fixes(start, &context, &findings)?
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
                &coordinates.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>(),
                DEFAULT_RESOLVE_DEPTH,
            )?
            .into_iter()
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        classpath.sort();
        classpath.dedup();
        Ok(classpath)
    }

    fn run_formatter(
        &self,
        java_binary: &Path,
        classpath: &[PathBuf],
        style: JavaFormatStyle,
        file: &Path,
    ) -> Result<String, DevToolsError> {
        let mut command = Command::new(java_binary);
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
        Ok(String::from_utf8(output.stdout)? )
    }
}

#[derive(Debug)]
pub struct FormatReport {
    pub project: ProjectBuildConfig,
    pub checked: bool,
    pub files_scanned: usize,
    pub changed_files: Vec<PathBuf>,
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
    fn load(start: &Path, resolver: &MavenResolver) -> Result<Self, DevToolsError> {
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
            let packages = collect_audit_packages(resolver, &workspace.external_dependencies, &root_to_members)?;
            return Ok(Self {
                root_dir: workspace.root_dir,
                packages,
            });
        }

        let config_path = find_jot_toml(start)?.ok_or_else(|| DevToolsError::AuditInvariant("could not find jot.toml".to_owned()))?;
        let project = load_project_build_config(start)?;
        let project_name = project.module_name.clone().unwrap_or_else(|| project.name.clone());
        let mut root_to_members = BTreeMap::<String, BTreeSet<String>>::new();
        for dependency in &project.dependencies {
            root_to_members
                .entry(dependency.clone())
                .or_default()
                .insert(project_name.clone());
        }
        Ok(Self {
            root_dir: config_path.parent().unwrap_or(start).to_path_buf(),
            packages: collect_audit_packages(resolver, &project.dependencies, &root_to_members)?,
        })
    }
}

fn collect_audit_packages(
    resolver: &MavenResolver,
    roots: &[String],
    root_to_members: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeMap<String, AuditPackageContext>, DevToolsError> {
    let mut packages = BTreeMap::<String, AuditPackageContext>::new();

    for root in roots {
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
            .entry((finding.package.group.clone(), finding.package.artifact.clone()))
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

    let config_path = find_jot_toml(start)?.ok_or_else(|| DevToolsError::AuditInvariant("could not find jot.toml".to_owned()))?;
    fixed += update_project_dependencies(&config_path, &by_package)?;
    let project = load_project_build_config(start)?;
    let lockfile = project.project_root.join("jot.lock");
    rewrite_lockfile(start, &project.project_root, &project.dependencies, &lockfile)?;
    Ok(fixed)
}

fn rewrite_lockfile(
    start: &Path,
    _root: &Path,
    inputs: &[String],
    output_path: &Path,
) -> Result<(), DevToolsError> {
    let config_path = find_workspace_root_jot_toml(start)?
        .or_else(|| find_jot_toml(start).ok().flatten());
    let Some(_root_config) = config_path else {
        return Ok(());
    };
    let paths = jot_cache::JotPaths::new().map_err(|error| DevToolsError::AuditInvariant(error.to_string()))?;
    let resolver = MavenResolver::new(paths)?;
    let lockfile = resolver.resolve_lockfile(inputs, DEFAULT_RESOLVE_DEPTH)?;
    fs::write(output_path, toml::to_string_pretty(&lockfile)?)?;
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
        let keys = table.iter().map(|(key, _)| key.to_owned()).collect::<Vec<_>>();
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
    if let Some(severity) = vuln
        .affected
        .iter()
        .find_map(|item| item.ecosystem_specific.as_ref().and_then(|value| value.severity.clone()))
        .or_else(|| {
            vuln.affected
                .iter()
                .find_map(|item| item.database_specific.as_ref().and_then(|value| value.severity.clone()))
        })
    {
        return parse_severity(&severity);
    }

    AuditSeverity::Unknown
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
    affected: Vec<OsvAffected>,
}

#[derive(Debug, Deserialize)]
struct OsvAffected {
    package: Option<OsvAffectedPackage>,
    #[serde(default)]
    ranges: Vec<OsvAffectedRange>,
    ecosystem_specific: Option<OsvSeverityHolder>,
    database_specific: Option<OsvSeverityHolder>,
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
    use super::{parse_severity, rewrite_coordinate, AuditSeverity, DEFAULT_PMD_RULESET};
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
    fn bundled_ruleset_references_java_categories() {
        assert!(DEFAULT_PMD_RULESET.contains("category/java/bestpractices.xml"));
        assert!(DEFAULT_PMD_RULESET.contains("category/java/errorprone.xml"));
    }
}