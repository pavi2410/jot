use fs2::FileExt;
use jot_cache::JotPaths;
use quick_xml::de::from_str;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;

use crate::coordinate::MavenCoordinate;
use crate::errors::ResolverError;
use crate::models::{MavenDependency, MavenMetadata, MavenParent, MavenProject};
use crate::versions::{
    is_property_version_expression, needs_dynamic_version_resolution, resolve_best_version,
    resolve_version_from_metadata,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    pub coordinate: MavenCoordinate,
    pub path: PathBuf,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    pub roots: Vec<MavenCoordinate>,
    pub package: Vec<LockedPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

struct CoordinateCacheKey {
    group: String,
    artifact: String,
    version: Option<String>,
    classifier_suffix: String,
}

impl CoordinateCacheKey {
    fn from_coordinate(coordinate: &MavenCoordinate) -> Self {
        let classifier_suffix = coordinate
            .classifier
            .as_deref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();

        Self {
            group: jot_common::sanitize_for_filename(&coordinate.group),
            artifact: jot_common::sanitize_for_filename(&coordinate.artifact),
            version: coordinate
                .version
                .as_deref()
                .map(jot_common::sanitize_for_filename),
            classifier_suffix: jot_common::sanitize_for_filename(&classifier_suffix),
        }
    }

    fn version_for_pom<'a>(
        &'a self,
        coordinate: &MavenCoordinate,
    ) -> Result<&'a str, ResolverError> {
        self.version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForPom(coordinate.to_string()))
    }

    fn version_for_artifact<'a>(
        &'a self,
        coordinate: &MavenCoordinate,
    ) -> Result<&'a str, ResolverError> {
        self.version
            .as_deref()
            .ok_or_else(|| ResolverError::MissingVersionForArtifact(coordinate.to_string()))
    }

    fn artifact_basename(&self, version: &str) -> String {
        format!(
            "jar-{}-{}-{}{}",
            self.group, self.artifact, version, self.classifier_suffix
        )
    }
}

struct InterpolationContext<'a> {
    properties: &'a BTreeMap<String, String>,
}

impl<'a> InterpolationContext<'a> {
    fn new(properties: &'a BTreeMap<String, String>) -> Self {
        Self { properties }
    }

    fn value(&self, input: &str) -> String {
        interpolate_value(input, self.properties)
    }

    fn optional_ref(&self, input: Option<&String>) -> Option<String> {
        input.map(|value| self.value(value))
    }
}

#[derive(Debug)]
pub struct MavenResolver {
    client: Client,
    paths: JotPaths,
    offline: bool,
}

impl MavenResolver {
    const METADATA_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

    pub fn new(paths: JotPaths) -> Result<Self, ResolverError> {
        Ok(Self {
            client: Client::builder().build()?,
            paths,
            offline: jot_common::offline_mode_enabled(),
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

    pub fn latest_available_version(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<Option<String>, ResolverError> {
        let metadata = self.fetch_metadata(coordinate)?;
        Ok(metadata.versioning.as_ref().and_then(resolve_best_version))
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
            let dependency_coordinate = dependency_coordinate(&dependency);

            if !include_classpath_scope(scope.as_deref()) {
                out.push(tree_entry(
                    depth,
                    dependency_coordinate,
                    scope,
                    optional,
                    Some("scope omitted"),
                ));
                continue;
            }

            if inherited_exclusions
                .contains(&(dependency.group.clone(), dependency.artifact.clone()))
            {
                out.push(tree_entry(
                    depth,
                    dependency_coordinate,
                    scope,
                    optional,
                    Some("excluded"),
                ));
                continue;
            }

            if optional && depth > 1 {
                out.push(tree_entry(
                    depth,
                    dependency_coordinate,
                    scope,
                    optional,
                    Some("optional omitted"),
                ));
                continue;
            }

            let Some(next_coordinate) = self.resolve_dependency_coordinate(&dependency)? else {
                out.push(tree_entry(
                    depth,
                    dependency_coordinate,
                    scope,
                    optional,
                    Some("unresolved version"),
                ));
                continue;
            };

            let key = next_coordinate.to_string();
            if seen.contains(&key) {
                out.push(tree_entry(
                    depth,
                    next_coordinate,
                    scope,
                    optional,
                    Some("cycle detected"),
                ));
                continue;
            }

            seen.insert(key);
            out.push(tree_entry(
                depth,
                next_coordinate.clone(),
                scope,
                optional,
                None,
            ));
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
                    let interpolated = InterpolationContext::new(&properties).value(&value);
                    properties.insert(name, interpolated);
                }
            }

            let interpolation = InterpolationContext::new(&properties);

            if let Some(management) = project.dependency_management {
                for dependency in management.dependencies.dependency {
                    let Some(group) = interpolation.optional_ref(dependency.group_id.as_ref())
                    else {
                        continue;
                    };
                    let Some(artifact) =
                        interpolation.optional_ref(dependency.artifact_id.as_ref())
                    else {
                        continue;
                    };

                    let scope = interpolation.optional_ref(dependency.scope.as_ref());
                    let packaging = interpolation.optional_ref(dependency.packaging.as_ref());

                    if scope.as_deref() == Some("import") && packaging.as_deref() == Some("pom") {
                        if let Some(version) =
                            interpolation.optional_ref(dependency.version.as_ref())
                        {
                            let imported = self.build_effective_model(
                                &MavenCoordinate {
                                    group: group.clone(),
                                    artifact: artifact.clone(),
                                    version: Some(version),
                                    classifier: interpolation
                                        .optional_ref(dependency.classifier.as_ref()),
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
                        managed_versions.insert((group, artifact), interpolation.value(&version));
                    }
                }
            }

            let dependencies = project
                .dependencies
                .map(|deps| {
                    deps.dependency
                        .into_iter()
                        .filter_map(|dependency| {
                            let group = interpolation.optional_ref(dependency.group_id.as_ref())?;
                            let artifact =
                                interpolation.optional_ref(dependency.artifact_id.as_ref())?;
                            let version = dependency
                                .version
                                .as_ref()
                                .map(|value| interpolation.value(value))
                                .or_else(|| {
                                    managed_versions
                                        .get(&(group.clone(), artifact.clone()))
                                        .cloned()
                                });

                            Some(ResolvedDependency {
                                group,
                                artifact,
                                version,
                                classifier: interpolation
                                    .optional_ref(dependency.classifier.as_ref()),
                                scope: interpolation.optional_ref(dependency.scope.as_ref()),
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
        if cache_path.is_file() && (self.offline || is_cache_usable(cache_path, ttl)?) {
            return Ok(fs::read_to_string(cache_path)?);
        }

        if self.offline {
            return Err(ResolverError::OfflineCacheMiss {
                url: url.to_owned(),
                cache_path: cache_path.to_path_buf(),
            });
        }

        let _lock = CacheWriteLock::acquire(&self.cache_lock_path(cache_path))?;
        if cache_path.is_file() && is_cache_usable(cache_path, ttl)? {
            return Ok(fs::read_to_string(cache_path)?);
        }

        let body = self.client.get(url).send()?.error_for_status()?.text()?;
        write_text_atomic(cache_path, &body)?;
        Ok(body)
    }

    fn cache_lock_path(&self, target: &Path) -> PathBuf {
        self.paths.locks_dir().join(format!(
            "resolver-{}.lock",
            jot_common::sanitize_for_filename(&target.to_string_lossy())
        ))
    }

    fn artifact_lock_path(&self, coordinate: &MavenCoordinate) -> PathBuf {
        self.paths.locks_dir().join(format!(
            "artifact-{}.lock",
            jot_common::sanitize_for_filename(&coordinate.to_string())
        ))
    }

    fn metadata_cache_path(&self, coordinate: &MavenCoordinate) -> PathBuf {
        let key = CoordinateCacheKey::from_coordinate(coordinate);
        self.paths
            .resolve_cache_dir()
            .join(format!("maven-metadata-{}-{}.xml", key.group, key.artifact,))
    }

    fn pom_cache_path(&self, coordinate: &MavenCoordinate) -> Result<PathBuf, ResolverError> {
        let key = CoordinateCacheKey::from_coordinate(coordinate);
        let version = key.version_for_pom(coordinate)?;

        Ok(self.paths.resolve_cache_dir().join(format!(
            "pom-{}-{}-{}.xml",
            key.group, key.artifact, version,
        )))
    }

    fn artifact_cache_path(&self, coordinate: &MavenCoordinate) -> Result<PathBuf, ResolverError> {
        let key = CoordinateCacheKey::from_coordinate(coordinate);
        let version = key.version_for_artifact(coordinate)?;

        Ok(self
            .paths
            .downloads_dir()
            .join(format!("{}.jar", key.artifact_basename(version))))
    }

    fn artifact_checksum_cache_path(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<PathBuf, ResolverError> {
        let key = CoordinateCacheKey::from_coordinate(coordinate);
        let version = key.version_for_artifact(coordinate)?;

        Ok(self
            .paths
            .resolve_cache_dir()
            .join(format!("{}.sha256", key.artifact_basename(version))))
    }

    fn cache_artifact_and_hash(
        &self,
        coordinate: &MavenCoordinate,
    ) -> Result<String, ResolverError> {
        let artifact_path = self.artifact_cache_path(coordinate)?;
        let _lock = CacheWriteLock::acquire(&self.artifact_lock_path(coordinate))?;

        if artifact_path.is_file() {
            let actual_checksum = sha256_file(&artifact_path)?;
            if self.offline {
                return Ok(actual_checksum);
            }

            let expected_checksum = self.fetch_artifact_checksum(coordinate)?;
            if expected_checksum
                .as_ref()
                .is_none_or(|expected| expected == &actual_checksum)
            {
                return Ok(actual_checksum);
            }

            fs::remove_file(&artifact_path)?;
        }

        if self.offline {
            return Err(ResolverError::OfflineArtifactMissing {
                coordinate: coordinate.to_string(),
                path: artifact_path,
            });
        }

        let expected_checksum = self.fetch_artifact_checksum(coordinate)?;

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

        if self.offline {
            if cache_path.is_file() {
                let body = fs::read_to_string(cache_path)?;
                return Ok(normalize_checksum_response(&body));
            }
            return Ok(None);
        }

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
        if self.offline {
            return Err(ResolverError::OfflineDownloadRequired(
                coordinate.to_string(),
            ));
        }

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

fn tree_entry(
    depth: usize,
    coordinate: MavenCoordinate,
    scope: Option<String>,
    optional: bool,
    note: Option<&str>,
) -> TreeEntry {
    TreeEntry {
        depth,
        coordinate,
        scope,
        optional,
        note: note.map(str::to_owned),
    }
}

fn dependency_coordinate(dependency: &ResolvedDependency) -> MavenCoordinate {
    MavenCoordinate {
        group: dependency.group.clone(),
        artifact: dependency.artifact.clone(),
        version: dependency.version.clone(),
        classifier: dependency.classifier.clone(),
    }
}

pub(crate) struct CacheWriteLock {
    file: fs::File,
}

impl CacheWriteLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, ResolverError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock_exclusive()
            .map_err(|source| ResolverError::LockAcquisition {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self { file })
    }
}

impl Drop for CacheWriteLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub(crate) fn write_text_atomic(path: &Path, body: &str) -> Result<(), ResolverError> {
    let parent = path.parent().ok_or_else(|| {
        ResolverError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no parent directory", path.display()),
        ))
    })?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(body.as_bytes())?;
    temp_file.flush()?;

    if path.exists() {
        fs::remove_file(path)?;
    }

    temp_file
        .persist(path)
        .map_err(|error| ResolverError::Io(error.error))?;
    Ok(())
}

pub(crate) fn include_classpath_scope(scope: Option<&str>) -> bool {
    !matches!(scope, Some("test" | "provided" | "import"))
}

pub(crate) fn interpolate_value(input: &str, properties: &BTreeMap<String, String>) -> String {
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

pub(crate) fn dependency_exclusions(
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

pub(crate) fn relocation_target(
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

pub(crate) fn sha256_file(path: &Path) -> Result<String, ResolverError> {
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

pub(crate) fn normalize_checksum_response(input: &str) -> Option<String> {
    input
        .split_whitespace()
        .find(|token| token.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|token| token.to_ascii_lowercase())
}

pub(crate) fn is_cache_usable(path: &Path, ttl: Option<Duration>) -> Result<bool, ResolverError> {
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
