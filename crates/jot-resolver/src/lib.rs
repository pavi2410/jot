use jot_cache::JotPaths;
use quick_xml::de::from_str;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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
    paths: JotPaths,
}

impl MavenResolver {
    const METADATA_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

    pub fn new(paths: JotPaths) -> Result<Self, ResolverError> {
        Ok(Self {
            client: Client::builder().build()?,
            paths,
        })
    }

    pub fn resolve_coordinate(&self, input: &str) -> Result<MavenCoordinate, ResolverError> {
        let coordinate = MavenCoordinate::parse(input)?;
        if coordinate.version.is_some() {
            return Ok(coordinate);
        }

        let metadata_xml = self.fetch_metadata_xml(&coordinate)?;
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
        let dependencies = self.fetch_direct_dependencies(&coordinate)?;

        Ok((coordinate, dependencies))
    }

    pub fn resolve_dependency_tree(
        &self,
        input: &str,
        max_depth: usize,
    ) -> Result<Vec<TreeEntry>, ResolverError> {
        let root = self.resolve_coordinate(input)?;
        let mut entries = vec![TreeEntry {
            depth: 0,
            coordinate: root.clone(),
            scope: None,
            optional: false,
            note: None,
        }];
        let mut seen = HashSet::new();
        seen.insert(root.to_string());
        self.walk_dependencies(&root, 1, max_depth, &mut seen, &mut entries)?;
        Ok(entries)
    }

    pub fn resolve_lockfile(
        &self,
        inputs: &[String],
        max_depth: usize,
    ) -> Result<Lockfile, ResolverError> {
        let mut roots = Vec::new();
        let mut packages = BTreeSet::new();

        for input in inputs {
            let root = self.resolve_coordinate(input)?;
            roots.push(root.clone());
            packages.insert(root.clone());

            let tree = self.resolve_dependency_tree(input, max_depth)?;
            for entry in tree.into_iter().skip(1) {
                if entry.note.as_deref() == Some("unresolved version") {
                    continue;
                }
                if entry.coordinate.version.is_some() {
                    packages.insert(entry.coordinate);
                }
            }
        }

        roots.sort();

        Ok(Lockfile {
            version: 1,
            roots,
            package: packages
                .into_iter()
                .map(|coordinate| LockedPackage {
                    group: coordinate.group,
                    artifact: coordinate.artifact,
                    version: coordinate.version.expect("locked package version"),
                })
                .collect(),
        })
    }

    fn walk_dependencies(
        &self,
        coordinate: &MavenCoordinate,
        depth: usize,
        max_depth: usize,
        seen: &mut HashSet<String>,
        out: &mut Vec<TreeEntry>,
    ) -> Result<(), ResolverError> {
        if depth > max_depth {
            return Ok(());
        }

        let dependencies = self.fetch_direct_dependencies(coordinate)?;
        for dependency in dependencies {
            let scope = dependency.scope.clone();
            let optional = dependency.optional;

            let Some(next_coordinate) = dependency.to_coordinate()? else {
                out.push(TreeEntry {
                    depth,
                    coordinate: MavenCoordinate {
                        group: dependency.group,
                        artifact: dependency.artifact,
                        version: dependency.version,
                    },
                    scope,
                    optional,
                    note: Some("unresolved version".to_owned()),
                });
                continue;
            };

            let key = next_coordinate.to_string();
            if seen.contains(&key) {
                out.push(TreeEntry {
                    depth,
                    coordinate: next_coordinate,
                    scope,
                    optional,
                    note: Some("cycle detected".to_owned()),
                });
                continue;
            }

            seen.insert(key);
            out.push(TreeEntry {
                depth,
                coordinate: next_coordinate.clone(),
                scope,
                optional,
                note: None,
            });
            self.walk_dependencies(&next_coordinate, depth + 1, max_depth, seen, out)?;
        }

        Ok(())
    }

    fn fetch_direct_dependencies(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<Vec<ResolvedDependency>, ResolverError> {
        let mut visiting = HashSet::new();
        let effective = self.build_effective_model(coordinate, &mut visiting)?;
        Ok(effective.dependencies)
    }

    fn build_effective_model(
        &self,
        coordinate: &MavenCoordinate,
        visiting: &mut HashSet<String>,
    ) -> Result<EffectivePomModel, ResolverError> {
        let key = coordinate.to_string();
        if !visiting.insert(key.clone()) {
            return Err(ResolverError::PomCycleDetected(key));
        }

        let model = (|| {
            let pom_xml = self.fetch_pom_xml(coordinate)?;
            let project: MavenProject = from_str(&pom_xml)?;

            let mut properties = BTreeMap::new();
            let mut managed_versions = BTreeMap::new();

            if let Some(parent_ref) = project.parent.clone() {
                let parent_coord = self.parent_to_coordinate(&parent_ref)?;
                let parent = self.build_effective_model(&parent_coord, visiting)?;
                properties.extend(parent.properties);
                managed_versions.extend(parent.managed_versions);
            }

            let project_group = project
                .group_id
                .clone()
                .or_else(|| project.parent.as_ref().and_then(|parent| parent.group_id.clone()))
                .unwrap_or_default();
            let project_version = project
                .version
                .clone()
                .or_else(|| project.parent.as_ref().and_then(|parent| parent.version.clone()))
                .unwrap_or_default();
            let project_artifact = project.artifact_id.clone().unwrap_or_default();

            properties.insert("project.groupId".to_owned(), project_group.clone());
            properties.insert("project.version".to_owned(), project_version.clone());
            properties.insert("project.artifactId".to_owned(), project_artifact.clone());

            if let Some(raw_properties) = project.properties {
                for (name, value) in raw_properties.entries {
                    let interpolated = interpolate_value(&value, &properties);
                    properties.insert(name, interpolated);
                }
            }

            if let Some(management) = project.dependency_management {
                for dependency in management.dependencies.dependency {
                    let Some(group) = dependency.group_id.map(|value| interpolate_value(&value, &properties)) else {
                        continue;
                    };
                    let Some(artifact) = dependency.artifact_id.map(|value| interpolate_value(&value, &properties)) else {
                        continue;
                    };

                    let scope = dependency
                        .scope
                        .as_ref()
                        .map(|value| interpolate_value(value, &properties));
                    let packaging = dependency
                        .packaging
                        .as_ref()
                        .map(|value| interpolate_value(value, &properties));

                    if scope.as_deref() == Some("import") && packaging.as_deref() == Some("pom") {
                        if let Some(version) = dependency
                            .version
                            .as_ref()
                            .map(|value| interpolate_value(value, &properties))
                        {
                            let imported = self.build_effective_model(
                                &MavenCoordinate {
                                    group: group.clone(),
                                    artifact: artifact.clone(),
                                    version: Some(version),
                                },
                                visiting,
                            )?;
                            for (key, value) in imported.managed_versions {
                                managed_versions.insert(key, value);
                            }
                        }
                        continue;
                    }

                    if let Some(version) = dependency.version {
                        managed_versions.insert(
                            (group, artifact),
                            interpolate_value(&version, &properties),
                        );
                    }
                }
            }

            let dependencies = project
                .dependencies
                .map(|deps| {
                    deps.dependency
                        .into_iter()
                        .filter_map(|dependency| {
                            let group = dependency
                                .group_id
                                .as_ref()
                                .map(|value| interpolate_value(value, &properties))?;
                            let artifact = dependency
                                .artifact_id
                                .as_ref()
                                .map(|value| interpolate_value(value, &properties))?;
                            let version = dependency.version.as_ref().map(|value| interpolate_value(value, &properties)).or_else(|| {
                                managed_versions
                                    .get(&(group.clone(), artifact.clone()))
                                    .cloned()
                            });

                            Some(ResolvedDependency {
                                group,
                                artifact,
                                version,
                                scope: dependency
                                    .scope
                                    .as_ref()
                                    .map(|value| interpolate_value(value, &properties)),
                                optional: dependency.optional.unwrap_or(false),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            Ok::<EffectivePomModel, ResolverError>(EffectivePomModel {
                properties,
                managed_versions,
                dependencies,
            })
        })();

        visiting.remove(&key);
        model
    }

    fn parent_to_coordinate(
        &self,
        parent: &MavenParent,
    ) -> Result<MavenCoordinate, ResolverError> {
        let group = parent
            .group_id
            .clone()
            .ok_or_else(|| ResolverError::InvalidParentPom("missing parent groupId".to_owned()))?;
        let artifact = parent
            .artifact_id
            .clone()
            .ok_or_else(|| ResolverError::InvalidParentPom("missing parent artifactId".to_owned()))?;
        let version = parent
            .version
            .clone()
            .ok_or_else(|| ResolverError::InvalidParentPom("missing parent version".to_owned()))?;

        Ok(MavenCoordinate {
            group,
            artifact,
            version: Some(version),
        })
    }

    fn fetch_metadata_xml(&self, coordinate: &MavenCoordinate) -> Result<String, ResolverError> {
        let url = coordinate.metadata_url();
        let cache_path = self.metadata_cache_path(coordinate);
        self.fetch_text_with_cache(&url, &cache_path, Some(Self::METADATA_CACHE_TTL))
    }

    fn fetch_pom_xml(&self, coordinate: &MavenCoordinate) -> Result<String, ResolverError> {
        let url = coordinate.pom_url()?;
        let cache_path = self.pom_cache_path(coordinate)?;
        self.fetch_text_with_cache(&url, &cache_path, None)
    }

    fn fetch_text_with_cache(
        &self,
        url: &str,
        cache_path: &Path,
        ttl: Option<Duration>,
    ) -> Result<String, ResolverError> {
        if cache_path.is_file() && is_cache_usable(cache_path, ttl)? {
            return Ok(fs::read_to_string(cache_path)?);
        }

        let body = self.client.get(url).send()?.error_for_status()?.text()?;
        fs::write(cache_path, &body)?;
        Ok(body)
    }

    fn metadata_cache_path(&self, coordinate: &MavenCoordinate) -> PathBuf {
        self.paths.resolve_cache_dir().join(format!(
            "maven-metadata-{}-{}.xml",
            sanitize_for_filename(&coordinate.group),
            sanitize_for_filename(&coordinate.artifact),
        ))
    }

    fn pom_cache_path(&self, coordinate: &MavenCoordinate) -> Result<PathBuf, ResolverError> {
        Ok(self.paths.resolve_cache_dir().join(format!(
            "pom-{}-{}-{}.xml",
            sanitize_for_filename(&coordinate.group),
            sanitize_for_filename(&coordinate.artifact),
            sanitize_for_filename(
                coordinate
                    .version
                    .as_deref()
                    .ok_or_else(|| ResolverError::MissingVersionForPom(coordinate.to_string()))?
            ),
        )))
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

impl ResolvedDependency {
    fn to_coordinate(&self) -> Result<Option<MavenCoordinate>, ResolverError> {
        let Some(version) = self.version.clone() else {
            return Ok(None);
        };

        if is_unresolved_version_expression(&version) {
            return Ok(None);
        }

        Ok(Some(MavenCoordinate {
            group: self.group.clone(),
            artifact: self.artifact.clone(),
            version: Some(version),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub depth: usize,
    pub coordinate: MavenCoordinate,
    pub scope: Option<String>,
    pub optional: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lockfile {
    pub version: u32,
    pub roots: Vec<MavenCoordinate>,
    pub package: Vec<LockedPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LockedPackage {
    pub group: String,
    pub artifact: String,
    pub version: String,
}

#[derive(Debug)]
struct EffectivePomModel {
    properties: BTreeMap<String, String>,
    managed_versions: BTreeMap<(String, String), String>,
    dependencies: Vec<ResolvedDependency>,
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

fn is_unresolved_version_expression(version: &str) -> bool {
    version.contains("${") || version.contains('[') || version.contains('(') || version.contains(',')
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

fn interpolate_value(input: &str, properties: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(start_offset) = input[cursor..].find("${") {
        let start = cursor + start_offset;
        result.push_str(&input[cursor..start]);

        let key_start = start + 2;
        let Some(end_offset) = input[key_start..].find('}') else {
            result.push_str(&input[start..]);
            return result;
        };
        let end = key_start + end_offset;
        let key = &input[key_start..end];

        if let Some(value) = properties.get(key) {
            result.push_str(value);
        } else {
            result.push_str(&input[start..=end]);
        }

        cursor = end + 1;
    }

    result.push_str(&input[cursor..]);
    result
}

fn is_cache_usable(path: &Path, ttl: Option<Duration>) -> Result<bool, ResolverError> {
    let Some(ttl) = ttl else {
        return Ok(true);
    };

    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    match modified.elapsed() {
        Ok(elapsed) => Ok(elapsed <= ttl),
        Err(_) => Ok(true),
    }
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
    #[serde(rename = "groupId")]
    group_id: Option<String>,
    #[serde(rename = "artifactId")]
    artifact_id: Option<String>,
    version: Option<String>,
    parent: Option<MavenParent>,
    properties: Option<MavenProperties>,
    #[serde(rename = "dependencyManagement")]
    dependency_management: Option<MavenDependencyManagement>,
    dependencies: Option<MavenDependencies>,
}

#[derive(Debug, Deserialize, Clone)]
struct MavenParent {
    #[serde(rename = "groupId")]
    group_id: Option<String>,
    #[serde(rename = "artifactId")]
    artifact_id: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MavenProperties {
    #[serde(flatten)]
    entries: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MavenDependencyManagement {
    dependencies: MavenDependencies,
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
    #[serde(rename = "type")]
    packaging: Option<String>,
    scope: Option<String>,
    optional: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error("invalid Maven coordinate {0}; expected group:artifact or group:artifact:version")]
    InvalidCoordinate(String),
    #[error("cache error: {0}")]
    Cache(#[from] jot_cache::CacheError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Xml(#[from] quick_xml::DeError),
    #[error("version metadata is missing for {0}")]
    MissingVersionMetadata(String),
    #[error("cannot compute POM URL because version is missing for {0}")]
    MissingVersionForPom(String),
    #[error("invalid parent POM declaration: {0}")]
    InvalidParentPom(String),
    #[error("detected a cycle while resolving POM model: {0}")]
    PomCycleDetected(String),
}

#[cfg(test)]
mod tests {
    use super::{
        Lockfile, MavenDependencies, MavenDependencyManagement, MavenParent,
        MavenCoordinate, MavenDependency, MavenProject, MavenVersioning, MavenVersions,
        ResolvedDependency, LockedPackage, is_cache_usable, is_stable_maven_version,
        is_unresolved_version_expression, interpolate_value,
        resolve_best_version,
    };
    use quick_xml::de::from_str;
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

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

            #[test]
            fn unresolved_version_expression_detection_matches_expected_cases() {
                assert!(is_unresolved_version_expression("${junit.version}"));
                assert!(is_unresolved_version_expression("[1.0,2.0)"));
                assert!(is_unresolved_version_expression("(,1.4.0]"));
                assert!(!is_unresolved_version_expression("1.2.3"));
            }

            #[test]
            fn dependency_to_coordinate_requires_literal_version() {
                let literal = ResolvedDependency {
                    group: "org.example".into(),
                    artifact: "demo".into(),
                    version: Some("1.0.0".into()),
                    scope: None,
                    optional: false,
                };
                assert_eq!(
                    literal
                        .to_coordinate()
                        .expect("literal version")
                        .expect("coordinate")
                        .to_string(),
                    "org.example:demo:1.0.0"
                );

                let managed = ResolvedDependency {
                    group: "org.example".into(),
                    artifact: "demo".into(),
                    version: Some("${demo.version}".into()),
                    scope: None,
                    optional: false,
                };
                assert!(managed.to_coordinate().expect("managed version").is_none());
            }

            #[test]
            fn cache_usability_respects_file_age_when_ttl_is_present() {
                let temp = tempdir().expect("tempdir");
                let file_path = temp.path().join("metadata.xml");
                fs::write(&file_path, "<metadata />").expect("write metadata");

                assert!(is_cache_usable(&file_path, Some(Duration::from_secs(60))).expect("fresh cache"));
                assert!(is_cache_usable(&file_path, None).expect("ttl-free cache"));
            }

            #[test]
            fn lockfile_packages_are_deterministic_and_deduplicated() {
                let lockfile = Lockfile {
                    version: 1,
                    roots: vec![MavenCoordinate {
                        group: "org.example".into(),
                        artifact: "demo".into(),
                        version: Some("1.0.0".into()),
                    }],
                    package: vec![
                        LockedPackage {
                            group: "b.group".into(),
                            artifact: "beta".into(),
                            version: "1.0.0".into(),
                        },
                        LockedPackage {
                            group: "a.group".into(),
                            artifact: "alpha".into(),
                            version: "2.0.0".into(),
                        },
                    ],
                };

                assert_eq!(lockfile.package[0].group, "b.group");
                assert_eq!(lockfile.package[1].group, "a.group");
            }

            #[test]
            fn interpolation_replaces_known_properties_and_keeps_unknown() {
                let mut properties = BTreeMap::new();
                properties.insert("junit.version".to_owned(), "5.11.0".to_owned());

                assert_eq!(
                    interpolate_value("org.junit:junit-bom:${junit.version}", &properties),
                    "org.junit:junit-bom:5.11.0"
                );
                assert_eq!(
                    interpolate_value("${missing.value}", &properties),
                    "${missing.value}"
                );
            }

            #[test]
            fn managed_versions_fill_dependency_versions() {
                let project = MavenProject {
                    group_id: Some("org.example".to_owned()),
                    artifact_id: Some("demo".to_owned()),
                    version: Some("1.0.0".to_owned()),
                    parent: Some(MavenParent {
                        group_id: Some("org.example".to_owned()),
                        artifact_id: Some("parent".to_owned()),
                        version: Some("1.0.0".to_owned()),
                    }),
                    properties: None,
                    dependency_management: Some(MavenDependencyManagement {
                        dependencies: MavenDependencies {
                            dependency: vec![MavenDependency {
                                group_id: Some("org.slf4j".to_owned()),
                                artifact_id: Some("slf4j-api".to_owned()),
                                version: Some("2.0.16".to_owned()),
                                packaging: None,
                                scope: None,
                                optional: None,
                            }],
                        },
                    }),
                    dependencies: Some(MavenDependencies {
                        dependency: vec![MavenDependency {
                            group_id: Some("org.slf4j".to_owned()),
                            artifact_id: Some("slf4j-api".to_owned()),
                            version: None,
                            packaging: None,
                            scope: None,
                            optional: None,
                        }],
                    }),
                };

                assert_eq!(
                    project
                        .dependency_management
                        .as_ref()
                        .expect("management")
                        .dependencies
                        .dependency[0]
                        .version
                        .as_deref(),
                    Some("2.0.16")
                );
            }
}