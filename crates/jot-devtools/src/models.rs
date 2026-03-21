use serde::Deserialize;

// ── PMD XML models ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PmdReport {
    #[serde(rename = "file", default)]
    pub files: Vec<PmdFile>,
    #[serde(rename = "error", default)]
    pub errors: Vec<PmdError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PmdFile {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "violation", default)]
    pub violations: Vec<PmdViolation>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PmdViolation {
    #[serde(rename = "@beginline")]
    pub begin_line: usize,
    #[serde(rename = "@endline")]
    pub end_line: usize,
    #[serde(rename = "@begincolumn")]
    pub begin_column: usize,
    #[serde(rename = "@endcolumn")]
    pub end_column: usize,
    #[serde(rename = "@rule")]
    pub rule: String,
    #[serde(rename = "@ruleset")]
    pub ruleset: String,
    #[serde(rename = "@priority")]
    pub priority: usize,
    #[serde(rename = "$text")]
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PmdError {
    #[serde(rename = "@filename")]
    pub filename: String,
    #[serde(rename = "@msg")]
    pub message: String,
}

// ── OSV API models ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub(crate) struct OsvBatchRequest {
    pub queries: Vec<OsvQuery>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct OsvQuery {
    pub version: String,
    pub package: OsvPackage,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct OsvPackage {
    pub ecosystem: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvBatchResponse {
    pub results: Vec<OsvBatchResult>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvBatchResult {
    #[serde(default)]
    pub vulns: Vec<OsvBatchVuln>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvBatchVuln {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvVulnerability {
    pub id: String,
    pub summary: Option<String>,
    #[serde(default)]
    pub severity: Vec<OsvSeverity>,
    pub ecosystem_specific: Option<OsvSeverityHolder>,
    pub database_specific: Option<OsvSeverityHolder>,
    #[serde(default)]
    pub affected: Vec<OsvAffected>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvAffected {
    pub package: Option<OsvAffectedPackage>,
    #[serde(default)]
    pub ranges: Vec<OsvAffectedRange>,
    #[serde(default)]
    pub severity: Vec<OsvSeverity>,
    pub ecosystem_specific: Option<OsvSeverityHolder>,
    pub database_specific: Option<OsvSeverityHolder>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvSeverity {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub score: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvAffectedPackage {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvAffectedRange {
    #[serde(default)]
    pub events: Vec<OsvRangeEvent>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvRangeEvent {
    pub fixed: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsvSeverityHolder {
    pub severity: Option<String>,
}
