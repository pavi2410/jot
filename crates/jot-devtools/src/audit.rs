use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use jot_config::{
    find_jot_toml, find_workspace_root_jot_toml, load_project_build_config,
    load_workspace_build_config, load_workspace_dependency_set,
};
use jot_resolver::{MavenCoordinate, MavenResolver, TreeEntry};
use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, Item, value};

use crate::models::{
    OsvBatchRequest, OsvBatchResponse, OsvPackage, OsvQuery, OsvSeverity, OsvVulnerability,
};
use crate::{DEFAULT_RESOLVE_DEPTH, DevTools, DevToolsError, count_bar, spinner};

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

// ── Internal context types ──────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct AuditContext {
    pub root_dir: PathBuf,
    pub packages: BTreeMap<String, AuditPackageContext>,
}

#[derive(Debug)]
pub(crate) struct AuditPackageContext {
    pub coordinate: MavenCoordinate,
    pub members: BTreeSet<String>,
    pub chains: Vec<Vec<MavenCoordinate>>,
}

impl AuditContext {
    pub fn load(
        start: &Path,
        resolver: &MavenResolver,
        progress: Option<&indicatif::ProgressBar>,
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

// ── DevTools impl ───────────────────────────────────────────────────────────

impl DevTools {
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
}

// ── Helper functions ────────────────────────────────────────────────────────

fn collect_audit_packages(
    resolver: &MavenResolver,
    roots: &[String],
    root_to_members: &BTreeMap<String, BTreeSet<String>>,
    progress: Option<&indicatif::ProgressBar>,
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
        .truncate(false)
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

pub(crate) fn rewrite_coordinate(
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

pub(crate) fn severity_for_vulnerability(vuln: &OsvVulnerability) -> AuditSeverity {
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

pub(crate) fn parse_severity(input: &str) -> AuditSeverity {
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

pub(crate) fn parse_cvss_score(value: &str, kind: Option<&str>) -> Option<f64> {
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
