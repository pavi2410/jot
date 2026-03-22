use std::collections::BTreeMap;

use jot_cache::JotPaths;
use jot_devtools::{AuditFinding, AuditReport, AuditSeverity, DevTools};
use jot_resolver::MavenResolver;
use jot_toolchain::ToolchainManager;

use crate::commands::render::{StatusTone, print_status_stdout, status_badge, stdout_color, style};

pub(crate) fn handle_audit(
    paths: JotPaths,
    fix: bool,
    ci: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ToolchainManager::new(paths.clone())?;
    let resolver = MavenResolver::new(paths)?;
    let devtools = DevTools::new(resolver, manager)?;
    let cwd = std::env::current_dir()?;
    let report = devtools.audit(&cwd, fix)?;

    if report.findings.is_empty() {
        print_status_stdout(
            "audit",
            StatusTone::Success,
            format!(
                "no vulnerabilities found across {} packages",
                report.packages_scanned
            ),
        );
        return Ok(());
    }

    let ci_failure = report
        .findings
        .iter()
        .any(|finding| ci && finding.severity.is_ci_failure());
    print_audit_report(&report, ci, stdout_color());

    if fix {
        print_status_stdout(
            "fix",
            StatusTone::Success,
            format!(
                "updated {} direct dependency declarations",
                report.fixed_dependencies
            ),
        );
    }

    if ci_failure {
        return Err("audit failed CI severity threshold".into());
    }

    Ok(())
}

fn print_audit_report(report: &AuditReport, ci: bool, color: bool) {
    let mut by_severity = BTreeMap::new();
    for severity in [
        AuditSeverity::Critical,
        AuditSeverity::High,
        AuditSeverity::Moderate,
        AuditSeverity::Low,
        AuditSeverity::Unknown,
    ] {
        by_severity.insert(severity, 0_usize);
    }
    for finding in &report.findings {
        *by_severity.entry(finding.severity).or_default() += 1;
    }

    print_status_stdout(
        "audit",
        StatusTone::Info,
        format!(
            "packages={} findings={} critical={} high={} moderate={} low={} unknown={}",
            report.packages_scanned,
            report.findings.len(),
            by_severity[&AuditSeverity::Critical],
            by_severity[&AuditSeverity::High],
            by_severity[&AuditSeverity::Moderate],
            by_severity[&AuditSeverity::Low],
            by_severity[&AuditSeverity::Unknown],
        ),
    );
    println!();

    for finding in &report.findings {
        print_audit_finding(finding, ci, color);
    }
}

fn print_audit_finding(finding: &AuditFinding, ci: bool, color: bool) {
    println!(
        "{} {}",
        severity_badge(finding.severity, color),
        finding.vuln_id
    );
    println!("  package: {}", finding.package);
    println!("  summary: {}", finding.summary);
    if let Some(version) = &finding.fixed_version {
        println!("  fixed version: {}", version);
    }
    if !finding.members.is_empty() {
        println!("  affected members: {}", finding.members.join(", "));
    }
    if ci && finding.severity.is_ci_failure() {
        println!(
            "  {}",
            style("ci gate: this finding fails --ci", StatusTone::Error, color)
        );
    }
    for (index, chain) in finding.chains.iter().enumerate() {
        let rendered = chain
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" -> ");
        println!("  path {}: {}", index + 1, rendered);
    }
    println!();
}

fn severity_badge(severity: AuditSeverity, color: bool) -> String {
    let tone = match severity {
        AuditSeverity::Critical => StatusTone::Error,
        AuditSeverity::High => StatusTone::Warning,
        AuditSeverity::Moderate => StatusTone::Info,
        AuditSeverity::Low => StatusTone::Accent,
        AuditSeverity::Unknown => StatusTone::Dim,
    };
    status_badge(severity.label(), tone, color)
}
