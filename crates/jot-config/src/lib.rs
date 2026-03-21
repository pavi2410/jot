use std::fs;
use std::path::{Path, PathBuf};

use jot_toolchain::{JavaToolchainRequest, JdkVendor};
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, Value, value};

#[derive(Debug, Deserialize)]
struct RawConfig {
    project: Option<RawProject>,
    dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    toolchains: Option<RawToolchains>,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    name: String,
    version: Option<String>,
    #[serde(rename = "main-class")]
    main_class: Option<String>,
    #[serde(rename = "source-dirs")]
    source_dirs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawToolchains {
    java: Option<RawJavaToolchain>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawJavaToolchain {
    Version(String),
    Detailed {
        version: String,
        vendor: Option<JdkVendor>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDependencySpec {
    Coords(String),
    Detailed {
        coords: Option<String>,
        path: Option<String>,
        catalog: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBuildConfig {
    pub config_path: PathBuf,
    pub project_root: PathBuf,
    pub name: String,
    pub version: String,
    pub main_class: Option<String>,
    pub source_dirs: Vec<PathBuf>,
    pub resource_dir: PathBuf,
    pub dependencies: Vec<String>,
    pub toolchain: Option<JavaToolchainRequest>,
}

pub fn find_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };

    loop {
        let candidate = current.join("jot.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn find_workspace_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };
    let mut found = None;

    loop {
        let candidate = current.join("jot.toml");
        if candidate.is_file() {
            found = Some(candidate);
        }

        if !current.pop() {
            return Ok(found);
        }
    }
}

pub fn read_toolchain_request(start: &Path) -> Result<Option<JavaToolchainRequest>, ConfigError> {
    let Some(path) = find_jot_toml(start)? else {
        return Ok(None);
    };

    let content = fs::read_to_string(path)?;
    let config: RawConfig = toml::from_str(&content)?;
    Ok(parse_toolchain_request(config.toolchains))
}

pub fn load_project_build_config(start: &Path) -> Result<ProjectBuildConfig, ConfigError> {
    let Some(config_path) = find_jot_toml(start)? else {
        return Err(ConfigError::ProjectConfigNotFound(start.to_path_buf()));
    };

    let project_root = config_path
        .parent()
        .ok_or_else(|| ConfigError::InvalidStartPath(config_path.clone()))?
        .to_path_buf();
    let content = fs::read_to_string(&config_path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let project = config
        .project
        .ok_or_else(|| ConfigError::MissingProjectSection(config_path.clone()))?;
    let source_dirs = project
        .source_dirs
        .unwrap_or_else(|| vec!["src/main/java".to_owned()])
        .into_iter()
        .map(|value| project_root.join(value))
        .collect();

    Ok(ProjectBuildConfig {
        config_path: config_path.clone(),
        project_root: project_root.clone(),
        name: project.name,
        version: project
            .version
            .ok_or_else(|| ConfigError::MissingProjectField {
                path: config_path.clone(),
                field: "version",
            })?,
        main_class: project.main_class,
        source_dirs,
        resource_dir: project_root.join("src/main/resources"),
        dependencies: extract_dependency_coordinates(config.dependencies)?,
        toolchain: parse_toolchain_request(config.toolchains),
    })
}

pub fn read_declared_dependencies(start: &Path) -> Result<Vec<String>, ConfigError> {
    let Some(path) = find_jot_toml(start)? else {
        return Ok(Vec::new());
    };

    let content = fs::read_to_string(path)?;
    let config: RawConfig = toml::from_str(&content)?;
    extract_dependency_coordinates(config.dependencies)
}

pub fn pin_java_toolchain(path: &Path, request: &JavaToolchainRequest) -> Result<(), ConfigError> {
    let content = fs::read_to_string(path)?;
    let mut document = content.parse::<DocumentMut>()?;

    let toolchains = document.entry("toolchains").or_insert(Item::Table(Table::new()));
    if !toolchains.is_table() {
        *toolchains = Item::Table(Table::new());
    }

    let java_item = match request.vendor {
        Some(vendor) => {
            let mut table = toml_edit::InlineTable::new();
            table.insert("version", Value::from(request.version.as_str()));
            table.insert("vendor", Value::from(vendor.to_string()));
            value(table)
        }
        None => value(request.version.as_str()),
    };
    toolchains["java"] = java_item;

    fs::write(path, document.to_string())?;
    Ok(())
}

fn parse_toolchain_request(
    toolchains: Option<RawToolchains>,
) -> Option<JavaToolchainRequest> {
    let toolchains = toolchains?;
    toolchains.java.map(|java| match java {
        RawJavaToolchain::Version(version) => JavaToolchainRequest {
            version,
            vendor: None,
        },
        RawJavaToolchain::Detailed { version, vendor } => JavaToolchainRequest {
            version,
            vendor,
        },
    })
}

fn extract_dependency_coordinates(
    dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
) -> Result<Vec<String>, ConfigError> {
    let mut result = Vec::new();

    for (name, spec) in dependencies.unwrap_or_default() {
        match spec {
            RawDependencySpec::Coords(coords) => result.push(coords),
            RawDependencySpec::Detailed {
                coords: Some(coords), ..
            } => result.push(coords),
            RawDependencySpec::Detailed {
                path: Some(_), ..
            } => {}
            RawDependencySpec::Detailed {
                catalog: Some(_), ..
            } => {
                return Err(ConfigError::UnsupportedDependencyDeclaration {
                    name,
                    reason: "catalog-based dependencies are not supported yet".to_owned(),
                });
            }
            RawDependencySpec::Detailed { .. } => {
                return Err(ConfigError::UnsupportedDependencyDeclaration {
                    name,
                    reason: "dependency declaration must include `coords`".to_owned(),
                });
            }
        }
    }

    Ok(result)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid start path: {0}")]
    InvalidStartPath(PathBuf),
    #[error("could not find jot.toml starting from {0}")]
    ProjectConfigNotFound(PathBuf),
    #[error("missing [project] section in {0}")]
    MissingProjectSection(PathBuf),
    #[error("missing [project].{field} in {path}")]
    MissingProjectField { path: PathBuf, field: &'static str },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse jot.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("failed to update jot.toml: {0}")]
    EditToml(#[from] toml_edit::TomlError),
    #[error("unsupported dependency declaration for `{name}`: {reason}")]
    UnsupportedDependencyDeclaration { name: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::{
		JavaToolchainRequest, find_jot_toml, find_workspace_jot_toml, load_project_build_config,
		pin_java_toolchain, read_declared_dependencies, read_toolchain_request,
    };
    use jot_toolchain::JdkVendor;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_nearest_config_in_parent_directory() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        let nested = root.join("nested").join("deeper");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(root.join("jot.toml"), "[toolchains]\njava = \"21\"\n").expect("write config");

        let path = find_jot_toml(&nested).expect("find config");
        assert_eq!(path, Some(root.join("jot.toml")));
    }

    #[test]
    fn finds_topmost_config_for_workspace_pin() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let member = workspace.join("member");
        let nested = member.join("src");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(workspace.join("jot.toml"), "[workspace]\nmembers = [\"member\"]\n")
            .expect("write workspace config");
        fs::write(member.join("jot.toml"), "[project]\nname = \"member\"\n")
            .expect("write member config");

        let path = find_workspace_jot_toml(&nested).expect("find workspace config");
        assert_eq!(path, Some(workspace.join("jot.toml")));
    }

    #[test]
    fn pins_java_toolchain_with_vendor_table() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(&config_path, "[project]\nname = \"demo\"\n").expect("write config");

        pin_java_toolchain(
            &config_path,
            &JavaToolchainRequest {
                version: "21".into(),
                vendor: Some(JdkVendor::Adoptium),
            },
        )
        .expect("pin toolchain");

        let request = read_toolchain_request(&config_path)
            .expect("read config")
            .expect("toolchain request");
        assert_eq!(request.version, "21");
        assert_eq!(request.vendor, Some(JdkVendor::Adoptium));
    }

    #[test]
    fn reads_explicit_coords_dependencies_from_config() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(
            &config_path,
            "[dependencies]\nslf4j = \"org.slf4j:slf4j-api:2.0.16\"\nserde = { coords = \"org.example:serde:1.0.0\" }\nlocal = { path = \"../local\" }\n",
        )
        .expect("write config");

        let dependencies = read_declared_dependencies(&config_path).expect("read dependencies");
        assert_eq!(
            dependencies,
            vec![
                "org.example:serde:1.0.0".to_owned(),
                "org.slf4j:slf4j-api:2.0.16".to_owned(),
            ]
        );
    }

    #[test]
    fn rejects_catalog_dependencies_for_lock_until_supported() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(
            &config_path,
            "[dependencies]\njunit = { catalog = \"junit\" }\n",
        )
        .expect("write config");

        let error = read_declared_dependencies(&config_path).expect_err("catalog should fail");
        assert!(error
            .to_string()
            .contains("catalog-based dependencies are not supported yet"));
    }

    #[test]
    fn loads_project_build_config_with_defaults() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(
            &config_path,
            "[project]\nname = \"demo\"\nversion = \"1.2.3\"\nmain-class = \"dev.demo.Main\"\n\n[toolchains]\njava = \"21\"\n\n[dependencies]\nslf4j = \"org.slf4j:slf4j-api:2.0.16\"\n",
        )
        .expect("write config");

        let config = load_project_build_config(temp.path()).expect("load project config");
        assert_eq!(config.name, "demo");
        assert_eq!(config.version, "1.2.3");
        assert_eq!(config.main_class.as_deref(), Some("dev.demo.Main"));
        assert_eq!(config.source_dirs, vec![temp.path().join("src/main/java")]);
        assert_eq!(config.resource_dir, temp.path().join("src/main/resources"));
        assert_eq!(config.dependencies, vec!["org.slf4j:slf4j-api:2.0.16".to_owned()]);
        assert_eq!(config.toolchain.expect("toolchain").version, "21");
    }
}