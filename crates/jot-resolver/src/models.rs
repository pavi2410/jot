use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
pub(crate) struct MavenMetadata {
    pub(crate) versioning: Option<MavenVersioning>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenVersioning {
    pub(crate) latest: Option<String>,
    pub(crate) release: Option<String>,
    pub(crate) versions: Option<MavenVersions>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenVersions {
    #[serde(default)]
    pub(crate) version: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenProject {
    #[serde(rename = "groupId")]
    pub(crate) group_id: Option<String>,
    #[serde(rename = "artifactId")]
    pub(crate) artifact_id: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) parent: Option<MavenParent>,
    pub(crate) properties: Option<BTreeMap<String, String>>,
    #[serde(rename = "dependencyManagement")]
    pub(crate) dependency_management: Option<MavenDependencyManagement>,
    #[serde(rename = "distributionManagement")]
    pub(crate) distribution_management: Option<MavenDistributionManagement>,
    pub(crate) dependencies: Option<MavenDependencies>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct MavenParent {
    #[serde(rename = "groupId")]
    pub(crate) group_id: Option<String>,
    #[serde(rename = "artifactId")]
    pub(crate) artifact_id: Option<String>,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenDependencyManagement {
    pub(crate) dependencies: MavenDependencies,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenDistributionManagement {
    pub(crate) relocation: Option<MavenRelocation>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenRelocation {
    #[serde(rename = "groupId")]
    pub(crate) group_id: Option<String>,
    #[serde(rename = "artifactId")]
    pub(crate) artifact_id: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) classifier: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenDependencies {
    #[serde(default)]
    pub(crate) dependency: Vec<MavenDependency>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenDependency {
    #[serde(rename = "groupId")]
    pub(crate) group_id: Option<String>,
    #[serde(rename = "artifactId")]
    pub(crate) artifact_id: Option<String>,
    pub(crate) version: Option<String>,
    #[serde(rename = "type")]
    pub(crate) packaging: Option<String>,
    pub(crate) classifier: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) optional: Option<bool>,
    pub(crate) exclusions: Option<MavenExclusions>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenExclusions {
    #[serde(default)]
    pub(crate) exclusion: Vec<MavenExclusion>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MavenExclusion {
    #[serde(rename = "groupId")]
    pub(crate) group_id: Option<String>,
    #[serde(rename = "artifactId")]
    pub(crate) artifact_id: Option<String>,
}
