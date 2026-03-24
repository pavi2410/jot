use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

use crate::errors::ResolverError;

pub(crate) const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MavenCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub classifier: Option<String>,
}

impl MavenCoordinate {
    pub fn parse(input: &str) -> Result<Self, ResolverError> {
        let parts = input.split(':').collect::<Vec<_>>();
        match parts.as_slice() {
            [group, artifact] => Ok(Self {
                group: (*group).to_owned(),
                artifact: (*artifact).to_owned(),
                version: None,
                classifier: None,
            }),
            [group, artifact, version] => Ok(Self {
                group: (*group).to_owned(),
                artifact: (*artifact).to_owned(),
                version: Some((*version).to_owned()),
                classifier: None,
            }),
            [group, artifact, version, classifier] => Ok(Self {
                group: (*group).to_owned(),
                artifact: (*artifact).to_owned(),
                version: Some((*version).to_owned()),
                classifier: Some((*classifier).to_owned()),
            }),
            _ => Err(ResolverError::InvalidCoordinate(input.to_owned())),
        }
    }

    pub fn with_version(&self, version: String) -> Self {
        Self {
            group: self.group.clone(),
            artifact: self.artifact.clone(),
            version: Some(version),
            classifier: self.classifier.clone(),
        }
    }

    /// Convert into a [`ResolvedCoordinate`], returning an error if the version is missing.
    pub fn into_resolved(self) -> Result<ResolvedCoordinate, ResolverError> {
        match self.version {
            Some(version) => Ok(ResolvedCoordinate {
                group: self.group,
                artifact: self.artifact,
                version,
                classifier: self.classifier,
            }),
            None => Err(ResolverError::MissingVersionForPom(format!(
                "{}:{}",
                self.group, self.artifact
            ))),
        }
    }

    pub(crate) fn metadata_url(&self) -> String {
        self.metadata_url_for(&repository_base_url())
    }

    pub(crate) fn metadata_url_for(&self, repository_base: &str) -> String {
        let group_path = self.group.replace('.', "/");
        format!(
            "{}/{}/{}/maven-metadata.xml",
            repository_base.trim_end_matches('/'),
            group_path,
            self.artifact
        )
    }

    pub(crate) fn pom_url(&self) -> Result<String, ResolverError> {
        self.pom_url_for(&repository_base_url())
    }

    pub(crate) fn pom_url_for(&self, repository_base: &str) -> Result<String, ResolverError> {
        let version = self
            .version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForPom(self.to_string()))?;
        let group_path = self.group.replace('.', "/");
        Ok(format!(
            "{}/{}/{}/{}/{}-{}.pom",
            repository_base.trim_end_matches('/'),
            group_path,
            self.artifact,
            version,
            self.artifact,
            version
        ))
    }

    pub(crate) fn jar_url(&self) -> Result<String, ResolverError> {
        self.jar_url_for(&repository_base_url())
    }

    pub(crate) fn jar_url_for(&self, repository_base: &str) -> Result<String, ResolverError> {
        let version = self
            .version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForArtifact(self.to_string()))?;
        let group_path = self.group.replace('.', "/");
        let classifier_suffix = self
            .classifier
            .as_deref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();

        Ok(format!(
            "{}/{}/{}/{}/{}-{}{}.jar",
            repository_base.trim_end_matches('/'),
            group_path,
            self.artifact,
            version,
            self.artifact,
            version,
            classifier_suffix,
        ))
    }

    pub(crate) fn jar_sha256_url(&self) -> Result<String, ResolverError> {
        Ok(format!("{}.sha256", self.jar_url()?))
    }
}

fn repository_base_url() -> String {
    std::env::var("JOT_MAVEN_REPOSITORY").unwrap_or_else(|_| MAVEN_CENTRAL.to_owned())
}

impl Display for MavenCoordinate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match (&self.version, &self.classifier) {
            (Some(version), Some(classifier)) => write!(
                formatter,
                "{}:{}:{}:{}",
                self.group, self.artifact, version, classifier
            ),
            (Some(version), None) => {
                write!(formatter, "{}:{}:{}", self.group, self.artifact, version)
            }
            (None, _) => write!(formatter, "{}:{}", self.group, self.artifact),
        }
    }
}

// ── Resolved coordinate (version always present) ─────────────────────────────

/// A Maven coordinate with a guaranteed version. Created by resolving a
/// [`MavenCoordinate`] or via [`MavenCoordinate::into_resolved`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResolvedCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
}

impl ResolvedCoordinate {
    /// Borrow as a [`MavenCoordinate`] (with `Some(version)`).
    pub fn as_coordinate(&self) -> MavenCoordinate {
        MavenCoordinate {
            group: self.group.clone(),
            artifact: self.artifact.clone(),
            version: Some(self.version.clone()),
            classifier: self.classifier.clone(),
        }
    }

    pub fn pom_url(&self) -> String {
        self.pom_url_for(&repository_base_url())
    }

    pub fn pom_url_for(&self, repository_base: &str) -> String {
        let group_path = self.group.replace('.', "/");
        format!(
            "{}/{}/{}/{}/{}-{}.pom",
            repository_base.trim_end_matches('/'),
            group_path,
            self.artifact,
            self.version,
            self.artifact,
            self.version
        )
    }

    pub fn jar_url(&self) -> String {
        self.jar_url_for(&repository_base_url())
    }

    pub fn jar_url_for(&self, repository_base: &str) -> String {
        let group_path = self.group.replace('.', "/");
        let classifier_suffix = self
            .classifier
            .as_deref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();

        format!(
            "{}/{}/{}/{}/{}-{}{}.jar",
            repository_base.trim_end_matches('/'),
            group_path,
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            classifier_suffix,
        )
    }

    pub fn jar_sha256_url(&self) -> String {
        format!("{}.sha256", self.jar_url())
    }
}

impl From<ResolvedCoordinate> for MavenCoordinate {
    fn from(resolved: ResolvedCoordinate) -> Self {
        Self {
            group: resolved.group,
            artifact: resolved.artifact,
            version: Some(resolved.version),
            classifier: resolved.classifier,
        }
    }
}

impl Display for ResolvedCoordinate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.classifier {
            Some(classifier) => write!(
                formatter,
                "{}:{}:{}:{}",
                self.group, self.artifact, self.version, classifier
            ),
            None => write!(
                formatter,
                "{}:{}:{}",
                self.group, self.artifact, self.version
            ),
        }
    }
}
