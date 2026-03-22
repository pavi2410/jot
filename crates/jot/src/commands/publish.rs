use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use jot_builder::{BuildOutput, JavaProjectBuilder};
use jot_cache::JotPaths;
use jot_config::{ProjectBuildConfig, PublishConfig, find_workspace_root_jot_toml};
use jot_resolver::{
    MavenCoordinate, MavenDependencies, MavenDependency, MavenDeveloper, MavenDevelopers,
    MavenLicense, MavenLicenses, MavenProject, MavenResolver, MavenScm,
};
use jot_toolchain::{InstalledJdk, ToolchainManager};
use quick_xml::se::to_string as to_xml_string;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use tempfile::TempDir;

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_publish(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
    repository: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    signing_key: Option<&str>,
    dry_run: bool,
    allow_snapshot: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let cwd = std::env::current_dir()?;
    let publish_target = PublishTarget::resolve(repository, username, password, dry_run)?;
    let signer = GpgSigner {
        signing_key: signing_key
            .map(ToOwned::to_owned)
            .or_else(|| std::env::var("JOT_PUBLISH_GPG_KEY").ok()),
    };

    if find_workspace_root_jot_toml(&cwd)?.is_some() {
        let output = builder.build_workspace(&cwd, module)?;
        let built_jars = output
            .modules
            .iter()
            .map(|item| {
                (
                    canonical_root(&item.build.project.project_root),
                    item.build.jar_path.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        for module_output in &output.modules {
            if let Some(requested_module) = module
                && module_output.module_name != requested_module
            {
                continue;
            }

            let path_dependency_jars =
                collect_path_dependency_jars(&builder, &module_output.build.project, &built_jars)?;

            publish_project(
                &module_output.build,
                &path_dependency_jars,
                publish_target.as_ref(),
                &signer,
                dry_run,
                allow_snapshot,
            )?;
            println!(
                "published {}:{}:{}",
                module_output
                    .build
                    .project
                    .group
                    .as_deref()
                    .unwrap_or("<missing-group>"),
                module_output.build.project.name,
                module_output.build.project.version
            );
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let output = builder.build(&cwd)?;
    let path_dependency_jars =
        collect_path_dependency_jars(&builder, &output.project, &BTreeMap::new())?;
    publish_project(
        &output,
        &path_dependency_jars,
        publish_target.as_ref(),
        &signer,
        dry_run,
        allow_snapshot,
    )?;
    println!(
        "published {}:{}:{}",
        output.project.group.as_deref().unwrap_or("<missing-group>"),
        output.project.name,
        output.project.version
    );
    Ok(())
}

fn publish_project(
    build: &BuildOutput,
    path_dependency_jars: &[PathBuf],
    publish_target: Option<&PublishTarget>,
    signer: &dyn ArtifactSigner,
    dry_run: bool,
    allow_snapshot: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = build.project.publish.clone().ok_or_else(|| {
        format!(
            "missing [publish] section in {}",
            build.project.config_path.display()
        )
    })?;
    let coordinate = publish_coordinate(&build.project, allow_snapshot)?;
    validate_publish_metadata(&coordinate, &metadata)?;

    let staged = stage_publish_artifacts(build, &coordinate, &metadata, path_dependency_jars)?;
    signer.sign(&staged.signing_inputs)?;
    write_sha256_sidecars(&staged.uploads)?;

    if dry_run {
        println!(
            "dry-run: staged publish bundle at {}",
            staged.stage_dir.display()
        );
        return Ok(());
    }

    let publish_target = publish_target
        .ok_or("missing publish repository; pass --repository or set JOT_PUBLISH_REPOSITORY")?;
    publish_target.ensure_version_absent(&coordinate, &staged.primary_artifact_name)?;
    publish_target.upload(&coordinate, &staged.uploads)?;
    Ok(())
}

fn publish_coordinate(
    project: &ProjectBuildConfig,
    allow_snapshot: bool,
) -> Result<MavenCoordinate, Box<dyn std::error::Error>> {
    let group = project.group.clone().ok_or_else(|| {
        format!(
            "{} is missing [project].group or inherited workspace group",
            project.config_path.display()
        )
    })?;
    let version = project.version.clone();
    if !allow_snapshot && version.to_ascii_uppercase().contains("SNAPSHOT") {
        return Err(format!(
            "refusing to publish snapshot version `{version}` without --allow-snapshot"
        )
        .into());
    }

    Ok(MavenCoordinate {
        group,
        artifact: project.name.clone(),
        version: Some(version),
        classifier: None,
    })
}

fn validate_publish_metadata(
    coordinate: &MavenCoordinate,
    metadata: &PublishConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = coordinate.version.as_deref().unwrap_or_default();
    if version.trim().is_empty() {
        return Err("publish version cannot be empty".into());
    }
    if metadata
        .license
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err("[publish].license is required".into());
    }
    if metadata
        .description
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err("[publish].description is required".into());
    }
    if metadata
        .url
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err("[publish].url is required".into());
    }
    if metadata
        .scm
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err("[publish].scm is required".into());
    }
    let developer = metadata
        .developer
        .as_ref()
        .ok_or("[publish].developer is required")?;
    if developer.name.trim().is_empty() {
        return Err("[publish].developer.name is required".into());
    }
    Ok(())
}

fn collect_path_dependency_jars(
    builder: &JavaProjectBuilder,
    project: &ProjectBuildConfig,
    built_jars: &BTreeMap<PathBuf, PathBuf>,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut jars = Vec::new();
    for dependency_root in &project.path_dependencies {
        let canonical = canonical_root(dependency_root);
        if let Some(path) = built_jars.get(&canonical) {
            jars.push(path.clone());
            continue;
        }
        let output = builder.build(dependency_root)?;
        jars.push(output.jar_path);
    }
    Ok(jars)
}

fn stage_publish_artifacts(
    build: &BuildOutput,
    coordinate: &MavenCoordinate,
    metadata: &PublishConfig,
    path_dependency_jars: &[PathBuf],
) -> Result<StagedPublish, Box<dyn std::error::Error>> {
    let version = coordinate
        .version
        .as_deref()
        .ok_or("publish coordinate missing version")?;
    let target_root = build.project.project_root.join("target").join("publish");
    let stage_dir = target_root.join(format!("{}-{}", coordinate.artifact, version));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)?;
    }
    fs::create_dir_all(&stage_dir)?;

    let main_jar_name = format!("{}-{}.jar", coordinate.artifact, version);
    let main_jar_path = stage_dir.join(&main_jar_name);
    fs::copy(&build.jar_path, &main_jar_path)?;

    let sources_jar_name = format!("{}-{}-sources.jar", coordinate.artifact, version);
    let sources_jar_path = stage_dir.join(&sources_jar_name);
    create_sources_jar(
        &build.installed_jdk,
        &build.project.source_dirs,
        &sources_jar_path,
    )?;

    let javadoc_jar_name = format!("{}-{}-javadoc.jar", coordinate.artifact, version);
    let javadoc_jar_path = stage_dir.join(&javadoc_jar_name);
    let mut javadoc_classpath = build
        .dependencies
        .iter()
        .map(|item| item.path.clone())
        .collect::<Vec<_>>();
    javadoc_classpath.extend(path_dependency_jars.iter().cloned());
    create_javadoc_jar(
        &build.installed_jdk,
        &build.project,
        &javadoc_classpath,
        &javadoc_jar_path,
    )?;

    let pom_name = format!("{}-{}.pom", coordinate.artifact, version);
    let pom_path = stage_dir.join(&pom_name);
    let pom = render_pom(&build.project, coordinate, metadata)?;
    fs::write(&pom_path, pom)?;

    let uploads = vec![
        main_jar_path.clone(),
        sources_jar_path.clone(),
        javadoc_jar_path.clone(),
        pom_path.clone(),
    ];

    Ok(StagedPublish {
        stage_dir,
        primary_artifact_name: main_jar_name,
        uploads: uploads.clone(),
        signing_inputs: uploads,
    })
}

fn create_sources_jar(
    installed_jdk: &InstalledJdk,
    source_dirs: &[PathBuf],
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let staging = TempDir::new()?;
    for source_dir in source_dirs {
        if !source_dir.exists() {
            continue;
        }
        copy_directory_contents(source_dir, staging.path())?;
    }
    package_directory_as_jar(installed_jdk, staging.path(), output_path)?;
    Ok(())
}

fn create_javadoc_jar(
    installed_jdk: &InstalledJdk,
    project: &ProjectBuildConfig,
    classpath_entries: &[PathBuf],
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let sources = collect_java_sources(&project.source_dirs)?;
    let doc_dir = TempDir::new()?;

    let mut command = Command::new(installed_jdk.javadoc_binary());
    command.arg("-quiet").arg("-d").arg(doc_dir.path());
    if !classpath_entries.is_empty() {
        command
            .arg("-classpath")
            .arg(std::env::join_paths(classpath_entries)?);
    }
    command.args(&sources);

    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "javadoc failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    package_directory_as_jar(installed_jdk, doc_dir.path(), output_path)?;
    Ok(())
}

fn package_directory_as_jar(
    installed_jdk: &InstalledJdk,
    source_dir: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = Command::new(installed_jdk.jar_binary())
        .arg("--create")
        .arg("--file")
        .arg(output_path)
        .arg("-C")
        .arg(source_dir)
        .arg(".")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "jar failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

fn render_pom(
    project: &ProjectBuildConfig,
    coordinate: &MavenCoordinate,
    metadata: &PublishConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    let version = coordinate
        .version
        .as_deref()
        .ok_or("publish coordinate missing version")?;
    let developer = metadata
        .developer
        .as_ref()
        .ok_or("[publish].developer is required")?;

    let dependencies = project
        .dependencies
        .iter()
        .map(|value| MavenCoordinate::parse(value))
        .collect::<Result<Vec<_>, _>>()?;
    let mut pom_dependencies = dependencies
        .into_iter()
        .map(PomDependency::from)
        .collect::<Vec<_>>();
    for path in &project.path_dependencies {
        let dependency_project = jot_config::load_project_build_config(path)?;
        pom_dependencies.push(PomDependency::from_project(&dependency_project)?);
    }
    pom_dependencies.sort_by(|left, right| {
        (&left.group_id, &left.artifact_id, &left.version).cmp(&(
            &right.group_id,
            &right.artifact_id,
            &right.version,
        ))
    });

    let pom_project = MavenProject {
        xml_namespace: Some("http://maven.apache.org/POM/4.0.0".to_owned()),
        xml_schema_namespace: Some("http://www.w3.org/2001/XMLSchema-instance".to_owned()),
        xml_schema_location: Some(
            "http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd"
                .to_owned(),
        ),
        model_version: Some("4.0.0".to_owned()),
        group_id: Some(coordinate.group.clone()),
        artifact_id: Some(coordinate.artifact.clone()),
        version: Some(version.to_owned()),
        packaging: Some("jar".to_owned()),
        name: Some(project.name.clone()),
        description: metadata.description.clone(),
        url: metadata.url.clone(),
        licenses: Some(MavenLicenses {
            license: vec![MavenLicense {
                name: metadata.license.clone(),
            }],
        }),
        scm: Some(MavenScm {
            url: metadata.scm.clone(),
            connection: metadata
                .scm
                .as_ref()
                .map(|value| format!("scm:git:{value}")),
        }),
        developers: Some(MavenDevelopers {
            developer: vec![MavenDeveloper {
                name: Some(developer.name.clone()),
                email: developer.email.clone(),
            }],
        }),
        dependencies: (!pom_dependencies.is_empty()).then_some(MavenDependencies {
            dependency: pom_dependencies
                .into_iter()
                .map(|dependency| MavenDependency {
                    group_id: Some(dependency.group_id),
                    artifact_id: Some(dependency.artifact_id),
                    version: Some(dependency.version),
                    classifier: dependency.classifier,
                    ..Default::default()
                })
                .collect(),
        }),
        ..Default::default()
    };

    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}\n",
        to_xml_string(&pom_project)?
    ))
}

fn write_sha256_sidecars(paths: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
    let mut pending = paths.to_vec();
    for path in paths {
        pending.push(signature_path_for(path));
    }

    for path in pending {
        let checksum = compute_sha256(&path)?;
        let checksum_path = checksum_path_for(&path);
        let mut file = fs::File::create(checksum_path)?;
        writeln!(file, "{checksum}")?;
    }
    Ok(())
}

fn compute_sha256(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65_536];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

fn find_gpg_binary() -> Result<String, Box<dyn std::error::Error>> {
    for candidate in ["gpg", "gpg2"] {
        let output = Command::new(candidate).arg("--version").output();
        if let Ok(output) = output
            && output.status.success()
        {
            return Ok(candidate.to_owned());
        }
    }
    Err("gpg is required for `jot publish` but was not found on PATH".into())
}

fn signature_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| format!("{}.asc", value.to_string_lossy()))
        .unwrap_or_else(|| "artifact.asc".to_owned());
    path.with_file_name(file_name)
}

fn checksum_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| format!("{}.sha256", value.to_string_lossy()))
        .unwrap_or_else(|| "artifact.sha256".to_owned());
    path.with_file_name(file_name)
}

fn copy_directory_contents(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory_contents(&source_path, &target_path)?;
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn collect_java_sources(
    source_dirs: &[PathBuf],
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for source_dir in source_dirs {
        collect_java_sources_in_dir(source_dir, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_java_sources_in_dir(
    path: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_java_sources_in_dir(&entry_path, files)?;
        } else if entry_path.extension().and_then(|value| value.to_str()) == Some("java") {
            files.push(entry_path);
        }
    }
    Ok(())
}

fn canonical_root(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Debug)]
struct StagedPublish {
    stage_dir: PathBuf,
    primary_artifact_name: String,
    uploads: Vec<PathBuf>,
    signing_inputs: Vec<PathBuf>,
}

#[derive(Debug)]
struct PomDependency {
    group_id: String,
    artifact_id: String,
    version: String,
    classifier: Option<String>,
}

impl PomDependency {
    fn from_project(project: &ProjectBuildConfig) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            group_id: project.group.clone().ok_or_else(|| {
                format!(
                    "path dependency {} is missing group for publish",
                    project.name
                )
            })?,
            artifact_id: project.name.clone(),
            version: project.version.clone(),
            classifier: None,
        })
    }
}

impl From<MavenCoordinate> for PomDependency {
    fn from(value: MavenCoordinate) -> Self {
        Self {
            group_id: value.group,
            artifact_id: value.artifact,
            version: value.version.unwrap_or_default(),
            classifier: value.classifier,
        }
    }
}

trait ArtifactSigner {
    fn sign(&self, artifacts: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug)]
struct GpgSigner {
    signing_key: Option<String>,
}

impl ArtifactSigner for GpgSigner {
    fn sign(&self, artifacts: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
        let gpg = find_gpg_binary()?;
        for artifact in artifacts {
            let signature_path = signature_path_for(artifact);
            if signature_path.exists() {
                fs::remove_file(&signature_path)?;
            }

            let mut command = Command::new(&gpg);
            command
                .arg("--batch")
                .arg("--yes")
                .arg("--armor")
                .arg("--detach-sign")
                .arg("--output")
                .arg(&signature_path);
            if let Some(signing_key) = self.signing_key.as_deref() {
                command.arg("--local-user").arg(signing_key);
            }
            command.arg(artifact);

            let output = command.output()?;
            if !output.status.success() {
                return Err(format!(
                    "gpg signing failed for {}: {}",
                    artifact.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                )
                .into());
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
enum PublishTarget {
    Local {
        root: PathBuf,
    },
    Http {
        base_url: String,
        username: Option<String>,
        password: Option<String>,
    },
}

impl PublishTarget {
    fn resolve(
        repository: Option<&str>,
        username: Option<&str>,
        password: Option<&str>,
        dry_run: bool,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let repository = repository
            .map(ToOwned::to_owned)
            .or_else(|| std::env::var("JOT_PUBLISH_REPOSITORY").ok());
        let username = username
            .map(ToOwned::to_owned)
            .or_else(|| std::env::var("JOT_PUBLISH_USERNAME").ok());
        let password = password
            .map(ToOwned::to_owned)
            .or_else(|| std::env::var("JOT_PUBLISH_PASSWORD").ok());

        let Some(repository) = repository else {
            return if dry_run {
                Ok(None)
            } else {
                Err(
                    "missing publish repository; pass --repository or set JOT_PUBLISH_REPOSITORY"
                        .into(),
                )
            };
        };

        if repository.starts_with("http://") || repository.starts_with("https://") {
            return Ok(Some(Self::Http {
                base_url: repository.trim_end_matches('/').to_owned(),
                username,
                password,
            }));
        }

        let path = repository
            .strip_prefix("file://")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(repository));
        Ok(Some(Self::Local { root: path }))
    }

    fn ensure_version_absent(
        &self,
        coordinate: &MavenCoordinate,
        primary_artifact_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Local { root } => {
                let artifact_path = root
                    .join(repository_relative_dir(coordinate)?)
                    .join(primary_artifact_name);
                if artifact_path.exists() {
                    return Err(format!(
                        "artifact already exists in repository: {}",
                        artifact_path.display()
                    )
                    .into());
                }
                Ok(())
            }
            Self::Http {
                base_url,
                username,
                password,
            } => {
                let client = http_client()?;
                let url = format!(
                    "{}/{}/{}",
                    base_url,
                    repository_relative_dir(coordinate)?,
                    primary_artifact_name
                );
                let mut request = client.head(&url);
                if let Some(username) = username {
                    request = request.basic_auth(username, password.clone());
                }
                let response = request.send()?;
                match response.status() {
                    StatusCode::OK => {
                        Err(format!("artifact already exists in repository: {url}").into())
                    }
                    StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => Ok(()),
                    status => {
                        Err(format!("failed to probe remote repository ({status}) at {url}").into())
                    }
                }
            }
        }
    }

    fn upload(
        &self,
        coordinate: &MavenCoordinate,
        uploads: &[PathBuf],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut files_to_upload = uploads.to_vec();
        for path in uploads {
            files_to_upload.push(signature_path_for(path));
        }
        let signature_files = files_to_upload.clone();
        for path in signature_files {
            files_to_upload.push(checksum_path_for(&path));
        }

        match self {
            Self::Local { root } => {
                let repo_dir = root.join(repository_relative_dir(coordinate)?);
                fs::create_dir_all(&repo_dir)?;
                for path in files_to_upload {
                    let file_name = path
                        .file_name()
                        .ok_or("publish artifact is missing file name")?;
                    fs::copy(&path, repo_dir.join(file_name))?;
                }
                Ok(())
            }
            Self::Http {
                base_url,
                username,
                password,
            } => {
                let client = http_client()?;
                let relative_dir = repository_relative_dir(coordinate)?;
                for path in files_to_upload {
                    let file_name = path
                        .file_name()
                        .ok_or("publish artifact is missing file name")?
                        .to_string_lossy();
                    let url = format!("{}/{}/{}", base_url, relative_dir, file_name);
                    let body = fs::read(&path)?;
                    let mut request = client.put(&url).body(body);
                    if let Some(username) = username {
                        request = request.basic_auth(username, password.clone());
                    }
                    let response = request.send()?;
                    if !response.status().is_success() {
                        return Err(
                            format!("upload failed for {url}: {}", response.status()).into()
                        );
                    }
                }
                Ok(())
            }
        }
    }
}

fn repository_relative_dir(
    coordinate: &MavenCoordinate,
) -> Result<String, Box<dyn std::error::Error>> {
    let version = coordinate
        .version
        .as_deref()
        .ok_or("publish coordinate missing version")?;
    Ok(format!(
        "{}/{}/{}",
        coordinate.group.replace('.', "/"),
        coordinate.artifact,
        version
    ))
}

fn http_client() -> Result<Client, Box<dyn std::error::Error>> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?)
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactSigner, PomDependency, PublishTarget, publish_project, render_pom,
        repository_relative_dir, signature_path_for,
    };
    use jot_builder::BuildOutput;
    use jot_cache::JotPaths;
    use jot_config::{ProjectBuildConfig, PublishConfig, PublishDeveloper};
    use jot_platform::Platform;
    use jot_resolver::MavenCoordinate;
    use jot_resolver::MavenResolver;
    use jot_toolchain::{InstalledJdk, JdkVendor};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;
    use time::OffsetDateTime;

    static MAVEN_REPO_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn repository_relative_dir_uses_maven_layout() {
        let coordinate = MavenCoordinate {
            group: "io.github.demo".into(),
            artifact: "demo".into(),
            version: Some("1.2.3".into()),
            classifier: None,
        };

        assert_eq!(
            repository_relative_dir(&coordinate).expect("relative dir"),
            "io/github/demo/demo/1.2.3"
        );
    }

    #[test]
    fn render_pom_includes_publish_metadata_and_dependencies() {
        let project = ProjectBuildConfig {
            config_path: PathBuf::from("jot.toml"),
            project_root: PathBuf::from("."),
            name: "demo".into(),
            version: "1.2.3".into(),
            group: Some("io.github.demo".into()),
            module_name: None,
            main_class: None,
            source_dirs: Vec::new(),
            test_source_dirs: Vec::new(),
            resource_dir: PathBuf::from("src/main/resources"),
            dependencies: vec!["org.slf4j:slf4j-api:2.0.16".into()],
            path_dependencies: Vec::new(),
            test_dependencies: Vec::new(),
            toolchain: None,
            publish: Some(PublishConfig {
                license: Some("Apache-2.0".into()),
                description: Some("Demo library".into()),
                url: Some("https://example.com/demo".into()),
                scm: Some("https://example.com/demo.git".into()),
                developer: Some(PublishDeveloper {
                    name: "Pavi".into(),
                    email: Some("pavi@example.com".into()),
                }),
            }),
            format: Default::default(),
            lint: Default::default(),
        };
        let coordinate = MavenCoordinate {
            group: "io.github.demo".into(),
            artifact: "demo".into(),
            version: Some("1.2.3".into()),
            classifier: None,
        };

        let pom = render_pom(
            &project,
            &coordinate,
            project.publish.as_ref().expect("publish config"),
        )
        .expect("render pom");

        assert!(pom.contains("<groupId>io.github.demo</groupId>"));
        assert!(pom.contains("<artifactId>demo</artifactId>"));
        assert!(pom.contains("<version>1.2.3</version>"));
        assert!(pom.contains("<name>Apache-2.0</name>"));
        assert!(pom.contains("<connection>scm:git:https://example.com/demo.git</connection>"));
        assert!(pom.contains("<artifactId>slf4j-api</artifactId>"));
        assert!(pom.contains("xmlns=\"http://maven.apache.org/POM/4.0.0\""));
    }

    #[test]
    fn pom_dependency_from_coordinate_preserves_classifier() {
        let dependency = PomDependency::from(MavenCoordinate {
            group: "org.example".into(),
            artifact: "demo".into(),
            version: Some("1.0.0".into()),
            classifier: Some("tests".into()),
        });

        assert_eq!(dependency.classifier.as_deref(), Some("tests"));
    }

    #[test]
    fn publish_roundtrip_to_local_repository_and_resolve_back() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let repo_root = temp.path().join("repo");
        let java_home = temp.path().join("fake-jdk");
        fs::create_dir_all(project_root.join("src/main/java/dev/demo")).expect("create sources");
        fs::create_dir_all(project_root.join("target")).expect("create target");
        fs::write(
            project_root.join("src/main/java/dev/demo/Main.java"),
            "package dev.demo; public class Main { public static void main(String[] args) {} }",
        )
        .expect("write source");
        fs::write(project_root.join("target/demo-1.2.3.jar"), "main-jar").expect("write main jar");
        create_fake_jdk(&java_home).expect("create fake jdk");

        let project = ProjectBuildConfig {
            config_path: project_root.join("jot.toml"),
            project_root: project_root.clone(),
            name: "demo".into(),
            version: "1.2.3".into(),
            group: Some("io.github.demo".into()),
            module_name: None,
            main_class: None,
            source_dirs: vec![project_root.join("src/main/java")],
            test_source_dirs: vec![project_root.join("src/test/java")],
            resource_dir: project_root.join("src/main/resources"),
            dependencies: Vec::new(),
            path_dependencies: Vec::new(),
            test_dependencies: Vec::new(),
            toolchain: None,
            publish: Some(PublishConfig {
                license: Some("Apache-2.0".into()),
                description: Some("Demo library".into()),
                url: Some("https://example.com/demo".into()),
                scm: Some("https://example.com/demo.git".into()),
                developer: Some(PublishDeveloper {
                    name: "Pavi".into(),
                    email: Some("pavi@example.com".into()),
                }),
            }),
            format: Default::default(),
            lint: Default::default(),
        };
        let build = BuildOutput {
            project,
            installed_jdk: InstalledJdk {
                vendor: JdkVendor::Adoptium,
                requested_version: "21".into(),
                release_name: "jdk-21.0.0-test".into(),
                semver: "21.0.0".into(),
                java_home: java_home.clone(),
                install_dir: temp.path().join("fake-install"),
                platform: Platform::current().expect("platform"),
                installed_at: OffsetDateTime::now_utc(),
            },
            dependencies: Vec::new(),
            classes_dir: project_root.join("target/classes"),
            jar_path: project_root.join("target/demo-1.2.3.jar"),
            fat_jar_path: None,
            fat_jar_warnings: Vec::new(),
        };

        let signer = TestSigner;
        let publish_target = PublishTarget::Local {
            root: repo_root.clone(),
        };
        publish_project(&build, &[], Some(&publish_target), &signer, false, false)
            .expect("publish to local repository");

        let relative_dir = repo_root.join("io/github/demo/demo/1.2.3");
        assert!(relative_dir.join("demo-1.2.3.jar").is_file());
        assert!(relative_dir.join("demo-1.2.3.jar.asc").is_file());
        assert!(relative_dir.join("demo-1.2.3.jar.sha256").is_file());
        assert!(relative_dir.join("demo-1.2.3.pom").is_file());

        let server = TestRepoServer::spawn(repo_root.clone()).expect("start test repo server");
        let _guard = MavenRepositoryEnvGuard::set(server.base_url());
        let paths = JotPaths::new().expect("jot paths");
        paths.ensure_exists().expect("ensure jot paths");
        let resolver = MavenResolver::new(paths).expect("resolver");
        let artifacts = resolver
            .resolve_artifacts(&["io.github.demo:demo:1.2.3".to_owned()], 1)
            .expect("resolve published artifact");

        assert_eq!(artifacts.len(), 1);
        let downloaded = fs::read_to_string(&artifacts[0].path).expect("read downloaded jar");
        assert_eq!(downloaded, "main-jar");
    }

    struct TestSigner;

    impl ArtifactSigner for TestSigner {
        fn sign(&self, artifacts: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
            for artifact in artifacts {
                fs::write(
                    signature_path_for(artifact),
                    format!("signed:{}", artifact.display()),
                )?;
            }
            Ok(())
        }
    }

    struct MavenRepositoryEnvGuard {
        _guard: MutexGuard<'static, ()>,
    }

    impl MavenRepositoryEnvGuard {
        fn set(value: String) -> Self {
            let lock = MAVEN_REPO_ENV_LOCK.lock().expect("env lock");
            unsafe {
                std::env::set_var("JOT_MAVEN_REPOSITORY", value);
            }
            Self { _guard: lock }
        }
    }

    impl Drop for MavenRepositoryEnvGuard {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("JOT_MAVEN_REPOSITORY");
            }
        }
    }

    struct TestRepoServer {
        base_url: String,
        stop: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestRepoServer {
        fn spawn(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let address = listener.local_addr()?;
            let stop = Arc::new(AtomicBool::new(false));
            let stop_flag = stop.clone();
            let handle = thread::spawn(move || {
                loop {
                    if stop_flag.load(Ordering::Relaxed) {
                        break;
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = serve_repo_request(&root, &mut stream);
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(Self {
                base_url: format!("http://{}", address),
                stop,
                handle: Some(handle),
            })
        }

        fn base_url(&self) -> String {
            self.base_url.clone()
        }
    }

    impl Drop for TestRepoServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn serve_repo_request(
        root: &Path,
        stream: &mut TcpStream,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = [0_u8; 8192];
        let bytes_read = stream.read(&mut buffer)?;
        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        let mut lines = request.lines();
        let request_line = lines.next().ok_or("missing request line")?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next().ok_or("missing method")?;
        let path = parts.next().ok_or("missing path")?;
        let relative = path.trim_start_matches('/');
        let file_path = root.join(relative);

        if !file_path.is_file() {
            stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")?;
            return Ok(());
        }

        let body = fs::read(&file_path)?;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes())?;
        if method != "HEAD" {
            stream.write_all(&body)?;
        }
        Ok(())
    }

    fn create_fake_jdk(java_home: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let bin_dir = java_home.join("bin");
        fs::create_dir_all(&bin_dir)?;
        write_executable(
            &bin_dir.join("jar"),
            "#!/bin/sh\nset -eu\nout=\"\"\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--file\" ]; then\n    out=\"$2\"\n    shift 2\n    continue\n  fi\n  shift\ndone\nmkdir -p \"$(dirname \"$out\")\"\nprintf 'fake-archive' > \"$out\"\n",
        )?;
        write_executable(
            &bin_dir.join("javadoc"),
            "#!/bin/sh\nset -eu\nout=\"\"\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"-d\" ]; then\n    out=\"$2\"\n    shift 2\n    continue\n  fi\n  shift\ndone\nmkdir -p \"$out\"\nprintf '<html>docs</html>' > \"$out/index.html\"\n",
        )?;
        Ok(())
    }

    fn write_executable(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
        fs::write(path, content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }
}
