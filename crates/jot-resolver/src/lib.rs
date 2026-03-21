use jot_cache::JotPaths;
use quick_xml::de::from_str;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;

const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct MavenCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub classifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    pub coordinate: MavenCoordinate,
    pub path: PathBuf,
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

    fn jar_url(&self) -> Result<String, ResolverError> {
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

    fn jar_sha256_url(&self) -> Result<String, ResolverError> {
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
        if let Some(version) = coordinate.version.as_deref() {
            if is_property_version_expression(version) {
                return Err(ResolverError::UnsupportedVersionExpression(
                    coordinate.to_string(),
                ));
            }

            if needs_dynamic_version_resolution(version) {
                let resolved = self.resolve_version_spec(&coordinate, version)?;
                return Ok(coordinate.with_version(resolved));
            }

            return Ok(coordinate);
        }

        let metadata = self.fetch_metadata(&coordinate)?;
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
        let inherited_exclusions = BTreeSet::new();
        self.walk_dependencies(
            &root,
            1,
            max_depth,
            &inherited_exclusions,
            &mut seen,
            &mut entries,
        )?;
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
                if entry.note.is_some() {
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
                .map(|coordinate| {
                    let sha256 = self.cache_artifact_and_hash(&coordinate)?;
                    Ok(LockedPackage {
                        group: coordinate.group,
                        artifact: coordinate.artifact,
                        version: coordinate.version.expect("locked package version"),
                        classifier: coordinate.classifier,
                        sha256,
                    })
                })
                .collect::<Result<Vec<_>, ResolverError>>()?,
        })
    }

    pub fn resolve_artifacts(
        &self,
        inputs: &[String],
        max_depth: usize,
    ) -> Result<Vec<ResolvedArtifact>, ResolverError> {
        let mut packages = BTreeSet::new();

        for input in inputs {
            let root = self.resolve_coordinate(input)?;
            packages.insert(root);

            for entry in self
                .resolve_dependency_tree(input, max_depth)?
                .into_iter()
                .skip(1)
            {
                if entry.note.is_some() || entry.coordinate.version.is_none() {
                    continue;
                }

                if include_classpath_scope(entry.scope.as_deref()) {
                    packages.insert(entry.coordinate);
                }
            }
        }

        packages
            .into_iter()
            .map(|coordinate| {
                let path = self.cache_artifact(&coordinate)?;
                Ok(ResolvedArtifact { coordinate, path })
            })
            .collect()
    }

    pub fn cache_artifact(&self, coordinate: &MavenCoordinate) -> Result<PathBuf, ResolverError> {
        self.cache_artifact_and_hash(coordinate)?;
        self.artifact_cache_path(coordinate)
    }

    fn walk_dependencies(
        &self,
        coordinate: &MavenCoordinate,
        depth: usize,
        max_depth: usize,
        inherited_exclusions: &BTreeSet<(String, String)>,
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

            if !include_classpath_scope(scope.as_deref()) {
                out.push(TreeEntry {
                    depth,
                    coordinate: MavenCoordinate {
                        group: dependency.group,
                        artifact: dependency.artifact,
                        version: dependency.version,
                        classifier: dependency.classifier,
                    },
                    scope,
                    optional,
                    note: Some("scope omitted".to_owned()),
                });
                continue;
            }

            if inherited_exclusions
                .contains(&(dependency.group.clone(), dependency.artifact.clone()))
            {
                out.push(TreeEntry {
                    depth,
                    coordinate: MavenCoordinate {
                        group: dependency.group,
                        artifact: dependency.artifact,
                        version: dependency.version,
                        classifier: dependency.classifier,
                    },
                    scope,
                    optional,
                    note: Some("excluded".to_owned()),
                });
                continue;
            }

            if optional && depth > 1 {
                out.push(TreeEntry {
                    depth,
                    coordinate: MavenCoordinate {
                        group: dependency.group,
                        artifact: dependency.artifact,
                        version: dependency.version,
                        classifier: dependency.classifier,
                    },
                    scope,
                    optional,
                    note: Some("optional omitted".to_owned()),
                });
                continue;
            }

            let Some(next_coordinate) = self.resolve_dependency_coordinate(&dependency)? else {
                out.push(TreeEntry {
                    depth,
                    coordinate: MavenCoordinate {
                        group: dependency.group,
                        artifact: dependency.artifact,
                        version: dependency.version,
                        classifier: dependency.classifier,
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
            let mut next_exclusions = inherited_exclusions.clone();
            next_exclusions.extend(dependency.exclusions);
            self.walk_dependencies(
                &next_coordinate,
                depth + 1,
                max_depth,
                &next_exclusions,
                seen,
                out,
            )?;
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

            if let Some(relocated) = relocation_target(&project, coordinate) {
                return self.build_effective_model(&relocated, visiting);
            }

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
                .or_else(|| {
                    project
                        .parent
                        .as_ref()
                        .and_then(|parent| parent.group_id.clone())
                })
                .unwrap_or_default();
            let project_version = project
                .version
                .clone()
                .or_else(|| {
                    project
                        .parent
                        .as_ref()
                        .and_then(|parent| parent.version.clone())
                })
                .unwrap_or_default();
            let project_artifact = project.artifact_id.clone().unwrap_or_default();

            properties.insert("project.groupId".to_owned(), project_group.clone());
            properties.insert("project.version".to_owned(), project_version.clone());
            properties.insert("project.artifactId".to_owned(), project_artifact.clone());

            if let Some(raw_properties) = project.properties {
                for (name, value) in raw_properties {
                    let interpolated = interpolate_value(&value, &properties);
                    properties.insert(name, interpolated);
                }
            }

            if let Some(management) = project.dependency_management {
                for dependency in management.dependencies.dependency {
                    let Some(group) = dependency
                        .group_id
                        .map(|value| interpolate_value(&value, &properties))
                    else {
                        continue;
                    };
                    let Some(artifact) = dependency
                        .artifact_id
                        .map(|value| interpolate_value(&value, &properties))
                    else {
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
                                    classifier: dependency
                                        .classifier
                                        .as_ref()
                                        .map(|value| interpolate_value(value, &properties)),
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
                        managed_versions
                            .insert((group, artifact), interpolate_value(&version, &properties));
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
                            let version = dependency
                                .version
                                .as_ref()
                                .map(|value| interpolate_value(value, &properties))
                                .or_else(|| {
                                    managed_versions
                                        .get(&(group.clone(), artifact.clone()))
                                        .cloned()
                                });

                            Some(ResolvedDependency {
                                group,
                                artifact,
                                version,
                                classifier: dependency
                                    .classifier
                                    .as_ref()
                                    .map(|value| interpolate_value(value, &properties)),
                                scope: dependency
                                    .scope
                                    .as_ref()
                                    .map(|value| interpolate_value(value, &properties)),
                                optional: dependency.optional.unwrap_or(false),
                                exclusions: dependency_exclusions(&dependency, &properties),
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

    fn resolve_dependency_coordinate(
        &self,
        dependency: &ResolvedDependency,
    ) -> Result<Option<MavenCoordinate>, ResolverError> {
        let Some(version) = dependency.version.as_deref() else {
            return Ok(None);
        };

        if is_property_version_expression(version) {
            return Ok(None);
        }

        let coordinate = MavenCoordinate {
            group: dependency.group.clone(),
            artifact: dependency.artifact.clone(),
            version: dependency.version.clone(),
            classifier: dependency.classifier.clone(),
        };

        if needs_dynamic_version_resolution(version) {
            let resolved = self.resolve_version_spec(&coordinate, version)?;
            return Ok(Some(coordinate.with_version(resolved)));
        }

        Ok(Some(coordinate))
    }

    fn resolve_version_spec(
        &self,
        coordinate: &MavenCoordinate,
        version_spec: &str,
    ) -> Result<String, ResolverError> {
        let metadata = self.fetch_metadata(coordinate)?;
        let versioning = metadata
            .versioning
            .as_ref()
            .ok_or_else(|| ResolverError::MissingVersionMetadata(coordinate.to_string()))?;

        resolve_version_from_metadata(versioning, version_spec).ok_or_else(|| {
            ResolverError::UnsupportedVersionExpression(format!("{} ({version_spec})", coordinate))
        })
    }

    fn fetch_metadata(&self, coordinate: &MavenCoordinate) -> Result<MavenMetadata, ResolverError> {
        let metadata_xml = self.fetch_metadata_xml(coordinate)?;
        Ok(from_str(&metadata_xml)?)
    }

    fn parent_to_coordinate(&self, parent: &MavenParent) -> Result<MavenCoordinate, ResolverError> {
        let group = parent
            .group_id
            .clone()
            .ok_or_else(|| ResolverError::InvalidParentPom("missing parent groupId".to_owned()))?;
        let artifact = parent.artifact_id.clone().ok_or_else(|| {
            ResolverError::InvalidParentPom("missing parent artifactId".to_owned())
        })?;
        let version = parent
            .version
            .clone()
            .ok_or_else(|| ResolverError::InvalidParentPom("missing parent version".to_owned()))?;

        Ok(MavenCoordinate {
            group,
            artifact,
            version: Some(version),
            classifier: None,
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

    fn artifact_cache_path(&self, coordinate: &MavenCoordinate) -> Result<PathBuf, ResolverError> {
        let version = coordinate
            .version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForArtifact(coordinate.to_string()))?;
        let classifier_suffix = coordinate
            .classifier
            .as_deref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();

        Ok(self.paths.downloads_dir().join(format!(
            "jar-{}-{}-{}{}.jar",
            sanitize_for_filename(&coordinate.group),
            sanitize_for_filename(&coordinate.artifact),
            sanitize_for_filename(version),
            sanitize_for_filename(&classifier_suffix),
        )))
    }

    fn artifact_checksum_cache_path(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<PathBuf, ResolverError> {
        let version = coordinate
            .version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForArtifact(coordinate.to_string()))?;
        let classifier_suffix = coordinate
            .classifier
            .as_deref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();

        Ok(self.paths.resolve_cache_dir().join(format!(
            "jar-{}-{}-{}{}.sha256",
            sanitize_for_filename(&coordinate.group),
            sanitize_for_filename(&coordinate.artifact),
            sanitize_for_filename(version),
            sanitize_for_filename(&classifier_suffix),
        )))
    }

    fn cache_artifact_and_hash(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<String, ResolverError> {
        let artifact_path = self.artifact_cache_path(coordinate)?;
        let expected_checksum = self.fetch_artifact_checksum(coordinate)?;

        if artifact_path.is_file() {
            let actual_checksum = sha256_file(&artifact_path)?;
            if expected_checksum
                .as_ref()
                .is_none_or(|expected| expected == &actual_checksum)
            {
                return Ok(actual_checksum);
            }
            fs::remove_file(&artifact_path)?;
        }

        self.download_artifact(coordinate, &artifact_path)?;
        let actual_checksum = sha256_file(&artifact_path)?;

        if let Some(expected_checksum) = expected_checksum
            && expected_checksum != actual_checksum
        {
            fs::remove_file(&artifact_path)?;
            return Err(ResolverError::ChecksumMismatch {
                coordinate: coordinate.to_string(),
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        Ok(actual_checksum)
    }

    fn fetch_artifact_checksum(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<Option<String>, ResolverError> {
        let checksum_url = coordinate.jar_sha256_url()?;
        let cache_path = self.artifact_checksum_cache_path(coordinate)?;

        match self.fetch_text_with_cache(&checksum_url, &cache_path, None) {
            Ok(body) => Ok(normalize_checksum_response(&body)),
            Err(ResolverError::Http(error))
                if error.status() == Some(reqwest::StatusCode::NOT_FOUND) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn download_artifact(
        &self,
        coordinate: &MavenCoordinate,
        destination: &Path,
    ) -> Result<(), ResolverError> {
        let url = coordinate.jar_url()?;
        let mut response = self.client.get(url).send()?.error_for_status()?;
        let mut temp_file = NamedTempFile::new_in(self.paths.downloads_dir())?;
        let mut buffer = [0_u8; 64 * 1024];

        loop {
            let bytes_read = response.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            temp_file.write_all(&buffer[..bytes_read])?;
        }

        temp_file.flush()?;
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        temp_file
            .persist(destination)
            .map_err(|error| ResolverError::Io(error.error))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDependency {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub classifier: Option<String>,
    pub scope: Option<String>,
    pub optional: bool,
    pub exclusions: BTreeSet<(String, String)>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classifier: Option<String>,
    pub sha256: String,
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

    if let Some(stable) = versioning.versions.as_ref().and_then(|versions| {
        versions
            .version
            .iter()
            .rev()
            .find(|version| is_stable_maven_version(version))
            .cloned()
    }) {
        return Some(stable);
    }

    versioning
        .versions
        .as_ref()
        .and_then(|versions| versions.version.last().cloned())
}

fn resolve_version_from_metadata(
    versioning: &MavenVersioning,
    version_spec: &str,
) -> Option<String> {
    if version_spec.eq_ignore_ascii_case("latest") {
        return versioning
            .latest
            .as_ref()
            .filter(|version| is_stable_maven_version(version))
            .cloned()
            .or_else(|| resolve_best_version(versioning));
    }

    if version_spec.eq_ignore_ascii_case("release") {
        return resolve_best_version(versioning);
    }

    let intervals = parse_version_spec(version_spec)?;
    let versions = versioning.versions.as_ref()?.version.as_slice();

    versions
        .iter()
        .rev()
        .find(|version| {
            is_stable_maven_version(version) && version_matches_any_range(version, &intervals)
        })
        .cloned()
        .or_else(|| {
            versions
                .iter()
                .rev()
                .find(|version| version_matches_any_range(version, &intervals))
                .cloned()
        })
}

fn parse_version_spec(spec: &str) -> Option<Vec<VersionRangeInterval>> {
    if !needs_dynamic_version_resolution(spec) || is_property_version_expression(spec) {
        return None;
    }

    if spec.starts_with('[') || spec.starts_with('(') {
        return parse_version_ranges(spec);
    }

    Some(vec![VersionRangeInterval {
        lower: Some((spec.to_owned(), true)),
        upper: Some((spec.to_owned(), true)),
    }])
}

fn parse_version_ranges(spec: &str) -> Option<Vec<VersionRangeInterval>> {
    let mut intervals = Vec::new();
    let mut start = 0;
    let chars = spec.char_indices().collect::<Vec<_>>();

    while start < spec.len() {
        let opening = spec[start..].chars().next()?;
        if opening != '[' && opening != '(' {
            return None;
        }

        let mut end = None;
        for (index, ch) in chars.iter().copied().filter(|(index, _)| *index > start) {
            if ch == ']' || ch == ')' {
                end = Some((index, ch));
                break;
            }
        }
        let (end_index, closing) = end?;
        let body = &spec[start + 1..end_index];
        let parts = body.splitn(2, ',').collect::<Vec<_>>();
        let lower = parts.first().copied().unwrap_or_default().trim();
        let upper = parts.get(1).copied().unwrap_or(lower).trim();

        let interval = if parts.len() == 1 {
            VersionRangeInterval {
                lower: if lower.is_empty() {
                    None
                } else {
                    Some((lower.to_owned(), opening == '['))
                },
                upper: if lower.is_empty() {
                    None
                } else {
                    Some((lower.to_owned(), closing == ']'))
                },
            }
        } else {
            VersionRangeInterval {
                lower: if lower.is_empty() {
                    None
                } else {
                    Some((lower.to_owned(), opening == '['))
                },
                upper: if upper.is_empty() {
                    None
                } else {
                    Some((upper.to_owned(), closing == ']'))
                },
            }
        };
        intervals.push(interval);

        start = end_index + 1;
        while spec[start..].starts_with(',') {
            start += 1;
            if start >= spec.len() {
                break;
            }
        }
    }

    if intervals.is_empty() {
        None
    } else {
        Some(intervals)
    }
}

fn version_matches_any_range(version: &str, intervals: &[VersionRangeInterval]) -> bool {
    intervals.iter().any(|interval| interval.matches(version))
}

fn needs_dynamic_version_resolution(version: &str) -> bool {
    version.eq_ignore_ascii_case("latest")
        || version.eq_ignore_ascii_case("release")
        || version.starts_with('[')
        || version.starts_with('(')
}

fn is_property_version_expression(version: &str) -> bool {
    version.contains("${")
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

fn include_classpath_scope(scope: Option<&str>) -> bool {
    !matches!(scope, Some("test" | "provided" | "import"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionRangeInterval {
    lower: Option<(String, bool)>,
    upper: Option<(String, bool)>,
}

impl VersionRangeInterval {
    fn matches(&self, version: &str) -> bool {
        let lower_matches = self.lower.as_ref().is_none_or(|(bound, inclusive)| {
            if *inclusive {
                compare_maven_versions(version, bound).is_ge()
            } else {
                compare_maven_versions(version, bound).is_gt()
            }
        });
        let upper_matches = self.upper.as_ref().is_none_or(|(bound, inclusive)| {
            if *inclusive {
                compare_maven_versions(version, bound).is_le()
            } else {
                compare_maven_versions(version, bound).is_lt()
            }
        });

        lower_matches && upper_matches
    }
}

fn compare_maven_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = tokenize_maven_version(left);
    let right_parts = tokenize_maven_version(right);
    let max_len = left_parts.len().max(right_parts.len());

    for index in 0..max_len {
        let left_part = left_parts
            .get(index)
            .cloned()
            .unwrap_or(VersionToken::Number(0));
        let right_part = right_parts
            .get(index)
            .cloned()
            .unwrap_or(VersionToken::Number(0));
        let ordering = left_part.cmp(&right_part);
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }

    std::cmp::Ordering::Equal
}

fn tokenize_maven_version(version: &str) -> Vec<VersionToken> {
    version
        .split(['.', '-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<u64>()
                .map(VersionToken::Number)
                .unwrap_or_else(|_| VersionToken::Text(part.to_ascii_lowercase()))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VersionToken {
    Number(u64),
    Text(String),
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

fn dependency_exclusions(
    dependency: &MavenDependency,
    properties: &BTreeMap<String, String>,
) -> BTreeSet<(String, String)> {
    dependency
        .exclusions
        .as_ref()
        .map(|exclusions| {
            exclusions
                .exclusion
                .iter()
                .filter_map(|entry| {
                    let group = entry
                        .group_id
                        .as_ref()
                        .map(|value| interpolate_value(value, properties))?;
                    let artifact = entry
                        .artifact_id
                        .as_ref()
                        .map(|value| interpolate_value(value, properties))?;
                    Some((group, artifact))
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default()
}

fn relocation_target(
    project: &MavenProject,
    coordinate: &MavenCoordinate,
) -> Option<MavenCoordinate> {
    let relocation = project
        .distribution_management
        .as_ref()?
        .relocation
        .as_ref()?;

    let group = relocation
        .group_id
        .clone()
        .or_else(|| project.group_id.clone())
        .unwrap_or_else(|| coordinate.group.clone());
    let artifact = relocation
        .artifact_id
        .clone()
        .or_else(|| project.artifact_id.clone())
        .unwrap_or_else(|| coordinate.artifact.clone());
    let version = relocation
        .version
        .clone()
        .or_else(|| project.version.clone())
        .or_else(|| coordinate.version.clone());
    let classifier = relocation
        .classifier
        .clone()
        .or_else(|| coordinate.classifier.clone());

    let relocated = MavenCoordinate {
        group,
        artifact,
        version,
        classifier,
    };

    if &relocated == coordinate {
        None
    } else {
        Some(relocated)
    }
}

fn sha256_file(path: &Path) -> Result<String, ResolverError> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

fn normalize_checksum_response(input: &str) -> Option<String> {
    input
        .split_whitespace()
        .find(|token| token.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|token| token.to_ascii_lowercase())
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
    properties: Option<BTreeMap<String, String>>,
    #[serde(rename = "dependencyManagement")]
    dependency_management: Option<MavenDependencyManagement>,
    #[serde(rename = "distributionManagement")]
    distribution_management: Option<MavenDistributionManagement>,
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
struct MavenDependencyManagement {
    dependencies: MavenDependencies,
}

#[derive(Debug, Deserialize)]
struct MavenDistributionManagement {
    relocation: Option<MavenRelocation>,
}

#[derive(Debug, Deserialize)]
struct MavenRelocation {
    #[serde(rename = "groupId")]
    group_id: Option<String>,
    #[serde(rename = "artifactId")]
    artifact_id: Option<String>,
    version: Option<String>,
    classifier: Option<String>,
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
    classifier: Option<String>,
    scope: Option<String>,
    optional: Option<bool>,
    exclusions: Option<MavenExclusions>,
}

#[derive(Debug, Deserialize)]
struct MavenExclusions {
    #[serde(default)]
    exclusion: Vec<MavenExclusion>,
}

#[derive(Debug, Deserialize)]
struct MavenExclusion {
    #[serde(rename = "groupId")]
    group_id: Option<String>,
    #[serde(rename = "artifactId")]
    artifact_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error(
        "invalid Maven coordinate {0}; expected group:artifact, group:artifact:version, or group:artifact:version:classifier"
    )]
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
    #[error("cannot compute artifact URL because version is missing for {0}")]
    MissingVersionForArtifact(String),
    #[error("unsupported or unresolvable version expression: {0}")]
    UnsupportedVersionExpression(String),
    #[error("invalid parent POM declaration: {0}")]
    InvalidParentPom(String),
    #[error("detected a cycle while resolving POM model: {0}")]
    PomCycleDetected(String),
    #[error("checksum mismatch for {coordinate}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        coordinate: String,
        expected: String,
        actual: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        LockedPackage, Lockfile, MavenCoordinate, MavenDependencies, MavenDependency,
        MavenDependencyManagement, MavenDistributionManagement, MavenExclusion, MavenExclusions,
        MavenParent, MavenProject, MavenRelocation, MavenVersioning, MavenVersions,
        ResolvedDependency, dependency_exclusions, include_classpath_scope, interpolate_value,
        is_cache_usable, is_property_version_expression, is_stable_maven_version,
        needs_dynamic_version_resolution, normalize_checksum_response, parse_version_spec,
        relocation_target, resolve_best_version, resolve_version_from_metadata, sha256_file,
        version_matches_any_range,
    };
    use quick_xml::de::from_str;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn parses_coordinates_with_optional_version() {
        let simple = MavenCoordinate::parse("org.junit.jupiter:junit-jupiter").expect("parse");
        assert_eq!(simple.group, "org.junit.jupiter");
        assert_eq!(simple.artifact, "junit-jupiter");
        assert_eq!(simple.version, None);
        assert_eq!(simple.classifier, None);

        let pinned =
            MavenCoordinate::parse("org.junit.jupiter:junit-jupiter:5.11.0").expect("parse");
        assert_eq!(pinned.version.as_deref(), Some("5.11.0"));

        let classified = MavenCoordinate::parse("org.junit.jupiter:junit-jupiter:5.11.0:sources")
            .expect("parse classified coordinate");
        assert_eq!(classified.classifier.as_deref(), Some("sources"));
        assert_eq!(
            classified.to_string(),
            "org.junit.jupiter:junit-jupiter:5.11.0:sources"
        );
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
        assert_eq!(
            resolve_best_version(&with_release).as_deref(),
            Some("1.9.0")
        );

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
        assert_eq!(
            resolve_best_version(&with_versions).as_deref(),
            Some("1.2.0")
        );

        let prefers_stable = MavenVersioning {
            latest: Some("2.0.0-RC1".into()),
            release: Some("2.0.0-M1".into()),
            versions: Some(MavenVersions {
                version: vec!["1.9.9".into(), "2.0.0-M1".into(), "2.0.0-RC1".into()],
            }),
        };
        assert_eq!(
            resolve_best_version(&prefers_stable).as_deref(),
            Some("1.9.9")
        );
    }

    #[test]
    fn include_classpath_scope_excludes_test_provided_and_import() {
        assert!(include_classpath_scope(None));
        assert!(include_classpath_scope(Some("compile")));
        assert!(include_classpath_scope(Some("runtime")));
        assert!(!include_classpath_scope(Some("test")));
        assert!(!include_classpath_scope(Some("provided")));
        assert!(!include_classpath_scope(Some("import")));
    }

    #[test]
    fn resolves_dynamic_versions_from_metadata() {
        let versioning = MavenVersioning {
            latest: Some("2.1.0".into()),
            release: Some("2.0.0".into()),
            versions: Some(MavenVersions {
                version: vec![
                    "1.0.0".into(),
                    "1.5.0".into(),
                    "1.9.9".into(),
                    "2.0.0-RC1".into(),
                    "2.0.0".into(),
                    "2.1.0".into(),
                ],
            }),
        };

        assert_eq!(
            resolve_version_from_metadata(&versioning, "LATEST").as_deref(),
            Some("2.1.0")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "RELEASE").as_deref(),
            Some("2.0.0")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "[1.5,2.0)").as_deref(),
            Some("1.9.9")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "(,1.0.0]").as_deref(),
            Some("1.0.0")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "[2.0.0,)").as_deref(),
            Some("2.1.0")
        );
    }

    #[test]
    fn parses_and_matches_union_ranges() {
        let intervals = parse_version_spec("(,1.0],[1.2,)").expect("parse ranges");
        assert!(version_matches_any_range("0.9", &intervals));
        assert!(!version_matches_any_range("1.1", &intervals));
        assert!(version_matches_any_range("1.2", &intervals));
        assert!(needs_dynamic_version_resolution("[1.0,2.0)"));
        assert!(!needs_dynamic_version_resolution("1.2.3"));
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
    fn property_version_expression_detection_matches_expected_cases() {
        assert!(is_property_version_expression("${junit.version}"));
        assert!(!is_property_version_expression("[1.0,2.0)"));
        assert!(!is_property_version_expression("(,1.4.0]"));
        assert!(!is_property_version_expression("1.2.3"));
    }

    #[test]
    fn dependency_to_coordinate_requires_literal_version() {
        let literal = ResolvedDependency {
            group: "org.example".into(),
            artifact: "demo".into(),
            version: Some("1.0.0".into()),
            classifier: Some("tests".into()),
            scope: None,
            optional: false,
            exclusions: BTreeSet::new(),
        };
        assert_eq!(literal.classifier.as_deref(), Some("tests"));

        let managed = ResolvedDependency {
            group: "org.example".into(),
            artifact: "demo".into(),
            version: Some("${demo.version}".into()),
            classifier: None,
            scope: None,
            optional: false,
            exclusions: BTreeSet::new(),
        };
        assert!(is_property_version_expression(
            managed.version.as_deref().expect("managed version")
        ));
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
                classifier: None,
            }],
            package: vec![
                LockedPackage {
                    group: "b.group".into(),
                    artifact: "beta".into(),
                    version: "1.0.0".into(),
                    classifier: None,
                    sha256: "abc123".into(),
                },
                LockedPackage {
                    group: "a.group".into(),
                    artifact: "alpha".into(),
                    version: "2.0.0".into(),
                    classifier: Some("sources".into()),
                    sha256: "def456".into(),
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
                        classifier: None,
                        scope: None,
                        optional: None,
                        exclusions: None,
                    }],
                },
            }),
            distribution_management: None,
            dependencies: Some(MavenDependencies {
                dependency: vec![MavenDependency {
                    group_id: Some("org.slf4j".to_owned()),
                    artifact_id: Some("slf4j-api".to_owned()),
                    version: None,
                    packaging: None,
                    classifier: Some("tests".to_owned()),
                    scope: None,
                    optional: None,
                    exclusions: None,
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

    #[test]
    fn parses_dependency_exclusions_and_interpolates_values() {
        let dependency = MavenDependency {
            group_id: Some("org.example".to_owned()),
            artifact_id: Some("consumer".to_owned()),
            version: Some("1.0.0".to_owned()),
            packaging: None,
            classifier: None,
            scope: None,
            optional: None,
            exclusions: Some(MavenExclusions {
                exclusion: vec![MavenExclusion {
                    group_id: Some("${excluded.group}".to_owned()),
                    artifact_id: Some("${excluded.artifact}".to_owned()),
                }],
            }),
        };

        let mut properties = BTreeMap::new();
        properties.insert("excluded.group".to_owned(), "org.slf4j".to_owned());
        properties.insert("excluded.artifact".to_owned(), "slf4j-api".to_owned());

        let exclusions = dependency_exclusions(&dependency, &properties);
        assert!(exclusions.contains(&("org.slf4j".to_owned(), "slf4j-api".to_owned())));
    }

    #[test]
    fn normalizes_checksum_sidecar_contents() {
        assert_eq!(
            normalize_checksum_response("ABCDEF0123456789  demo.jar\n"),
            Some("abcdef0123456789".to_owned())
        );
        assert_eq!(normalize_checksum_response(""), None);
    }

    #[test]
    fn sha256_file_hashes_contents() {
        let temp = tempdir().expect("tempdir");
        let file_path = temp.path().join("demo.jar");
        fs::write(&file_path, b"hello world").expect("write artifact");

        let checksum = sha256_file(&file_path).expect("sha256");
        assert_eq!(
            checksum,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn relocation_target_uses_relocation_fields_with_coordinate_fallbacks() {
        let project = MavenProject {
            group_id: Some("legacy.group".to_owned()),
            artifact_id: Some("legacy-artifact".to_owned()),
            version: Some("1.0.0".to_owned()),
            parent: None,
            properties: None,
            dependency_management: None,
            distribution_management: Some(MavenDistributionManagement {
                relocation: Some(MavenRelocation {
                    group_id: Some("modern.group".to_owned()),
                    artifact_id: Some("modern-artifact".to_owned()),
                    version: Some("2.0.0".to_owned()),
                    classifier: Some("sources".to_owned()),
                }),
            }),
            dependencies: None,
        };

        let resolved = relocation_target(
            &project,
            &MavenCoordinate {
                group: "legacy.group".to_owned(),
                artifact: "legacy-artifact".to_owned(),
                version: Some("1.0.0".to_owned()),
                classifier: None,
            },
        )
        .expect("relocation target");

        assert_eq!(
            resolved.to_string(),
            "modern.group:modern-artifact:2.0.0:sources"
        );
    }
}
