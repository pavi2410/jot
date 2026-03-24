use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ── Maven scope ─────────────────────────────────────────────────────────────

/// The scope of a Maven dependency, controlling when it appears on the classpath.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MavenScope {
    #[default]
    Compile,
    Runtime,
    Test,
    Provided,
    Import,
    System,
}

impl MavenScope {
    /// Returns `true` if the scope should be included on the compile/runtime classpath.
    pub fn is_classpath_visible(self) -> bool {
        !matches!(self, Self::Test | Self::Provided | Self::Import)
    }
}

impl fmt::Display for MavenScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile => f.write_str("compile"),
            Self::Runtime => f.write_str("runtime"),
            Self::Test => f.write_str("test"),
            Self::Provided => f.write_str("provided"),
            Self::Import => f.write_str("import"),
            Self::System => f.write_str("system"),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versioning: Option<MavenVersioning>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenVersioning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versions: Option<MavenVersions>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenVersions {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub version: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenProject {
    #[serde(rename = "@xmlns", skip_serializing_if = "Option::is_none")]
    pub xml_namespace: Option<String>,
    #[serde(rename = "@xmlns:xsi", skip_serializing_if = "Option::is_none")]
    pub xml_schema_namespace: Option<String>,
    #[serde(
        rename = "@xsi:schemaLocation",
        skip_serializing_if = "Option::is_none"
    )]
    pub xml_schema_location: Option<String>,
    #[serde(rename = "modelVersion", skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(rename = "groupId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "artifactId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packaging: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<MavenParent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, String>>,
    #[serde(rename = "dependencyManagement")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_management: Option<MavenDependencyManagement>,
    #[serde(rename = "distributionManagement")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution_management: Option<MavenDistributionManagement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub licenses: Option<MavenLicenses>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scm: Option<MavenScm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub developers: Option<MavenDevelopers>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<MavenDependencies>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenParent {
    #[serde(rename = "groupId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "artifactId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenDependencyManagement {
    pub dependencies: MavenDependencies,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenDistributionManagement {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relocation: Option<MavenRelocation>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenRelocation {
    #[serde(rename = "groupId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "artifactId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classifier: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenDependencies {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependency: Vec<MavenDependency>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenDependency {
    #[serde(rename = "groupId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "artifactId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packaging: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<MavenScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusions: Option<MavenExclusions>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenExclusions {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclusion: Vec<MavenExclusion>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenExclusion {
    #[serde(rename = "groupId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "artifactId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenLicenses {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub license: Vec<MavenLicense>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenLicense {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenScm {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenDevelopers {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub developer: Vec<MavenDeveloper>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MavenDeveloper {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}
