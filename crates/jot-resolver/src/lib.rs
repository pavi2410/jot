use quick_xml::de::from_str;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::fmt::{Display, Formatter};

const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
}

impl MavenCoordinate {
    pub fn parse(input: &str) -> Result<Self, ResolverError> {
        let parts = input.split(':').collect::<Vec<_>>();
        match parts.as_slice() {
            [group, artifact] => Ok(Self {
                group: (*group).to_owned(),
                artifact: (*artifact).to_owned(),
                version: None,
            }),
            [group, artifact, version] => Ok(Self {
                group: (*group).to_owned(),
                artifact: (*artifact).to_owned(),
                version: Some((*version).to_owned()),
            }),
            _ => Err(ResolverError::InvalidCoordinate(input.to_owned())),
        }
    }

    pub fn with_version(&self, version: String) -> Self {
        Self {
            group: self.group.clone(),
            artifact: self.artifact.clone(),
            version: Some(version),
        }
    }

    fn metadata_url(&self) -> String {
        let group_path = self.group.replace('.', "/");
        format!(
            "{}/{}/{}/maven-metadata.xml",
            MAVEN_CENTRAL, group_path, self.artifact
        )
    }

    fn pom_url(&self) -> Result<String, ResolverError> {
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
}

impl Display for MavenCoordinate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(version) = &self.version {
            write!(formatter, "{}:{}:{}", self.group, self.artifact, version)
        } else {
            write!(formatter, "{}:{}", self.group, self.artifact)
        }
    }
}

#[derive(Debug)]
pub struct MavenResolver {
    client: Client,
}

impl MavenResolver {
    pub fn new() -> Result<Self, ResolverError> {
        Ok(Self {
            client: Client::builder().build()?,
        })
    }

    pub fn resolve_coordinate(&self, input: &str) -> Result<MavenCoordinate, ResolverError> {
        let coordinate = MavenCoordinate::parse(input)?;
        if coordinate.version.is_some() {
            return Ok(coordinate);
        }

        let metadata_xml = self
            .client
            .get(coordinate.metadata_url())
            .send()?
            .error_for_status()?
            .text()?;
        let metadata: MavenMetadata = from_str(&metadata_xml)?;
        let version = metadata
            .versioning
            .as_ref()
            .and_then(resolve_best_version)
            .ok_or_else(|| ResolverError::MissingVersionMetadata(coordinate.to_string()))?;
        Ok(coordinate.with_version(version))
    }

    pub fn resolve_direct_dependencies(
        &self,
        input: &str,
    ) -> Result<(MavenCoordinate, Vec<ResolvedDependency>), ResolverError> {
        let coordinate = self.resolve_coordinate(input)?;
        let pom_url = coordinate.pom_url()?;
        let pom_xml = self
            .client
            .get(pom_url)
            .send()?
            .error_for_status()?
            .text()?;
        let project: MavenProject = from_str(&pom_xml)?;

        let dependencies = project
            .dependencies
            .map(|deps| {
                deps.dependency
                    .into_iter()
                    .filter(|dependency| dependency.group_id.is_some() && dependency.artifact_id.is_some())
                    .map(|dependency| ResolvedDependency {
                        group: dependency.group_id.unwrap_or_default(),
                        artifact: dependency.artifact_id.unwrap_or_default(),
                        version: dependency.version,
                        scope: dependency.scope,
                        optional: dependency.optional.unwrap_or(false),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok((coordinate, dependencies))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDependency {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub scope: Option<String>,
    pub optional: bool,
}

fn resolve_best_version(versioning: &MavenVersioning) -> Option<String> {
    if let Some(release) = &versioning.release
        && is_stable_maven_version(release)
    {
        return Some(release.clone());
    }

    if let Some(latest) = &versioning.latest
        && is_stable_maven_version(latest)
    {
        return Some(latest.clone());
    }

    if let Some(stable) = versioning
        .versions
        .as_ref()
        .and_then(|versions| {
            versions
                .version
                .iter()
                .rev()
                .find(|version| is_stable_maven_version(version))
                .cloned()
        })
    {
        return Some(stable);
    }

    versioning
        .versions
        .as_ref()
        .and_then(|versions| versions.version.last().cloned())
}

fn is_stable_maven_version(version: &str) -> bool {
    let lowered = version.to_ascii_lowercase();
    !lowered.contains("snapshot")
        && !lowered.contains("alpha")
        && !lowered.contains("beta")
        && !lowered.contains("rc")
        && !lowered.contains("milestone")
        && !lowered.contains("m")
}

#[derive(Debug, Deserialize)]
struct MavenMetadata {
    versioning: Option<MavenVersioning>,
}

#[derive(Debug, Deserialize)]
struct MavenVersioning {
    latest: Option<String>,
    release: Option<String>,
    versions: Option<MavenVersions>,
}

#[derive(Debug, Deserialize)]
struct MavenVersions {
    #[serde(default)]
    version: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MavenProject {
    dependencies: Option<MavenDependencies>,
}

#[derive(Debug, Deserialize)]
struct MavenDependencies {
    #[serde(default)]
    dependency: Vec<MavenDependency>,
}

#[derive(Debug, Deserialize)]
struct MavenDependency {
    #[serde(rename = "groupId")]
    group_id: Option<String>,
    #[serde(rename = "artifactId")]
    artifact_id: Option<String>,
    version: Option<String>,
    scope: Option<String>,
    optional: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error("invalid Maven coordinate {0}; expected group:artifact or group:artifact:version")]
    InvalidCoordinate(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("xml parse error: {0}")]
    Xml(#[from] quick_xml::DeError),
    #[error("version metadata is missing for {0}")]
    MissingVersionMetadata(String),
    #[error("cannot compute POM URL because version is missing for {0}")]
    MissingVersionForPom(String),
}

#[cfg(test)]
mod tests {
    use super::{
        MavenCoordinate, MavenDependency, MavenProject, MavenVersioning, MavenVersions,
        is_stable_maven_version, resolve_best_version,
    };
    use quick_xml::de::from_str;

    #[test]
    fn parses_coordinates_with_optional_version() {
        let simple = MavenCoordinate::parse("org.junit.jupiter:junit-jupiter").expect("parse");
        assert_eq!(simple.group, "org.junit.jupiter");
        assert_eq!(simple.artifact, "junit-jupiter");
        assert_eq!(simple.version, None);

        let pinned = MavenCoordinate::parse("org.junit.jupiter:junit-jupiter:5.11.0").expect("parse");
        assert_eq!(pinned.version.as_deref(), Some("5.11.0"));
    }

    #[test]
    fn best_version_prefers_release_then_latest_then_last_listed() {
        let with_release = MavenVersioning {
            latest: Some("2.0.0".into()),
            release: Some("1.9.0".into()),
            versions: Some(MavenVersions {
                version: vec!["1.0.0".into(), "1.9.0".into()],
            }),
        };
        assert_eq!(resolve_best_version(&with_release).as_deref(), Some("1.9.0"));

        let with_latest = MavenVersioning {
            latest: Some("2.0.0".into()),
            release: None,
            versions: None,
        };
        assert_eq!(resolve_best_version(&with_latest).as_deref(), Some("2.0.0"));

        let with_versions = MavenVersioning {
            latest: None,
            release: None,
            versions: Some(MavenVersions {
                version: vec!["1.0.0".into(), "1.1.0".into(), "1.2.0".into()],
            }),
        };
        assert_eq!(resolve_best_version(&with_versions).as_deref(), Some("1.2.0"));

        let prefers_stable = MavenVersioning {
            latest: Some("2.0.0-RC1".into()),
            release: Some("2.0.0-M1".into()),
            versions: Some(MavenVersions {
                version: vec!["1.9.9".into(), "2.0.0-M1".into(), "2.0.0-RC1".into()],
            }),
        };
        assert_eq!(resolve_best_version(&prefers_stable).as_deref(), Some("1.9.9"));
    }

    #[test]
    fn stable_version_filter_accepts_and_rejects_expected_formats() {
        assert!(is_stable_maven_version("1.2.3"));
        assert!(is_stable_maven_version("1.2.3.Final"));
        assert!(!is_stable_maven_version("1.2.3-SNAPSHOT"));
        assert!(!is_stable_maven_version("2.0.0-M1"));
        assert!(!is_stable_maven_version("2.0.0-RC1"));
    }

        #[test]
        fn parses_maven_dependencies_block_from_pom_xml() {
                let xml = r#"
                        <project>
                            <dependencies>
                                <dependency>
                                    <groupId>org.junit.jupiter</groupId>
                                    <artifactId>junit-jupiter-api</artifactId>
                                    <version>5.11.0</version>
                                    <scope>test</scope>
                                    <optional>false</optional>
                                </dependency>
                            </dependencies>
                        </project>
                "#;

                let project: MavenProject = from_str(xml).expect("parse pom");
                let dependencies = project.dependencies.expect("dependencies").dependency;
                assert_eq!(dependencies.len(), 1);
                let first: &MavenDependency = &dependencies[0];
                assert_eq!(first.group_id.as_deref(), Some("org.junit.jupiter"));
                assert_eq!(first.artifact_id.as_deref(), Some("junit-jupiter-api"));
                assert_eq!(first.version.as_deref(), Some("5.11.0"));
                assert_eq!(first.scope.as_deref(), Some("test"));
                assert_eq!(first.optional, Some(false));
        }
}