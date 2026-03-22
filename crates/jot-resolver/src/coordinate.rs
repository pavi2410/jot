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

    pub(crate) fn metadata_url(&self) -> String {
        let group_path = self.group.replace('.', "/");
        format!(
            "{}/{}/{}/maven-metadata.xml",
            MAVEN_CENTRAL, group_path, self.artifact
        )
    }

    pub(crate) fn pom_url(&self) -> Result<String, ResolverError> {
        let version = self
            .version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForPom(self.to_string()))?;
        let group_path = self.group.replace('.', "/");
        Ok(format!(
            "{}/{}/{}/{}/{}-{}.pom",
            MAVEN_CENTRAL, group_path, self.artifact, version, self.artifact, version
        ))
    }

    pub(crate) fn jar_url(&self) -> Result<String, ResolverError> {
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
            MAVEN_CENTRAL,
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
