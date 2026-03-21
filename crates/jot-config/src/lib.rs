use std::fs;
use std::path::{Path, PathBuf};

use jot_toolchain::{JavaToolchainRequest, JdkVendor};
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, Value, value};

#[derive(Debug, Clone, Deserialize)]
struct RawConfig {
    project: Option<RawProject>,
    workspace: Option<RawWorkspace>,
    dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    #[serde(rename = "test-dependencies")]
    test_dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    toolchains: Option<RawToolchains>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawProject {
    name: String,
    version: Option<String>,
    group: Option<String>,
    #[serde(rename = "main-class")]
    main_class: Option<String>,
    #[serde(rename = "source-dirs")]
    source_dirs: Option<Vec<String>>,
    #[serde(rename = "test-source-dirs")]
    test_source_dirs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawWorkspace {
    members: Vec<String>,
    group: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawToolchains {
    java: Option<RawJavaToolchain>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawJavaToolchain {
    Version(String),
    Detailed {
        version: String,
        vendor: Option<JdkVendor>,
    },
}

#[derive(Debug, Clone, Deserialize)]
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
    pub group: Option<String>,
    pub module_name: Option<String>,
    pub main_class: Option<String>,
    pub source_dirs: Vec<PathBuf>,
    pub test_source_dirs: Vec<PathBuf>,
    pub resource_dir: PathBuf,
    pub dependencies: Vec<String>,
    pub path_dependencies: Vec<PathBuf>,
    pub test_dependencies: Vec<String>,
    pub toolchain: Option<JavaToolchainRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBuildConfig {
    pub config_path: PathBuf,
    pub root_dir: PathBuf,
    pub group: Option<String>,
    pub toolchain: Option<JavaToolchainRequest>,
    pub members: Vec<WorkspaceMemberBuildConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemberBuildConfig {
    pub module_name: String,
    pub project: ProjectBuildConfig,
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

pub fn find_workspace_root_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
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
            let content = fs::read_to_string(&candidate)?;
            let config: RawConfig = toml::from_str(&content)?;
            if config.workspace.is_some() {
                return Ok(Some(candidate));
            }
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn read_toolchain_request(start: &Path) -> Result<Option<JavaToolchainRequest>, ConfigError> {
    let Some(path) = find_jot_toml(start)? else {
        return Ok(None);
    };

    let content = fs::read_to_string(&path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let inherited = inherited_workspace_context(path.parent().unwrap_or(start))?;
    Ok(parse_toolchain_request(config.toolchains).or(inherited.and_then(|ctx| ctx.toolchain)))
}

pub fn load_project_build_config(start: &Path) -> Result<ProjectBuildConfig, ConfigError> {
    let Some(config_path) = find_jot_toml(start)? else {
        return Err(ConfigError::ProjectConfigNotFound(start.to_path_buf()));
    };

    load_project_build_config_from_file(&config_path)
}

pub fn load_workspace_build_config(start: &Path) -> Result<Option<WorkspaceBuildConfig>, ConfigError> {
    let Some(workspace_config_path) = find_workspace_root_jot_toml(start)? else {
        return Ok(None);
    };

    let root_dir = workspace_config_path
        .parent()
        .ok_or_else(|| ConfigError::InvalidStartPath(workspace_config_path.clone()))?
        .to_path_buf();
    let root_content = fs::read_to_string(&workspace_config_path)?;
    let root_config: RawConfig = toml::from_str(&root_content)?;
    let workspace = root_config
        .workspace
        .ok_or_else(|| ConfigError::MissingWorkspaceSection(workspace_config_path.clone()))?;
    if workspace.members.is_empty() {
        return Err(ConfigError::InvalidWorkspaceConfig {
            path: workspace_config_path,
            reason: "[workspace].members must include at least one module".to_owned(),
        });
    }

    let root_toolchain = parse_toolchain_request(root_config.toolchains);
    let mut members = Vec::new();
    let mut seen_names = std::collections::BTreeSet::new();

    for member in workspace.members {
        let member_root = root_dir.join(&member);
        let member_config_path = member_root.join("jot.toml");
        if !member_config_path.is_file() {
            return Err(ConfigError::WorkspaceMemberNotFound(member_config_path));
        }

        let module_name = module_name_from_member(&member)?;
        if !seen_names.insert(module_name.clone()) {
            return Err(ConfigError::DuplicateWorkspaceModule(module_name));
        }

        let inherited = WorkspaceInheritance {
            group: workspace.group.clone(),
            toolchain: root_toolchain.clone(),
            module_name: Some(module_name.clone()),
        };
        let project = load_project_build_config_from_file_with_inheritance(
            &member_config_path,
            Some(inherited),
        )?;

        members.push(WorkspaceMemberBuildConfig {
            module_name,
            project,
        });
    }

    let member_roots = members
        .iter()
        .map(|member| {
            member
                .project
                .project_root
                .canonicalize()
                .unwrap_or_else(|_| member.project.project_root.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    for member in &members {
        for dependency in &member.project.path_dependencies {
            if !member_roots.contains(dependency) {
                return Err(ConfigError::PathDependencyOutsideWorkspace {
                    module: member.module_name.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    detect_workspace_path_cycles(&members)?;

    Ok(Some(WorkspaceBuildConfig {
        config_path: root_dir.join("jot.toml"),
        root_dir,
        group: workspace.group,
        toolchain: root_toolchain,
        members,
    }))
}

fn load_project_build_config_from_file(config_path: &Path) -> Result<ProjectBuildConfig, ConfigError> {
    let inherited = inherited_workspace_context(
        config_path
            .parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(config_path.to_path_buf()))?,
    )?;
    load_project_build_config_from_file_with_inheritance(config_path, inherited)
}

fn load_project_build_config_from_file_with_inheritance(
    config_path: &Path,
    inherited: Option<WorkspaceInheritance>,
) -> Result<ProjectBuildConfig, ConfigError> {
    let config_path = config_path.to_path_buf();

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
    let test_source_dirs = project
        .test_source_dirs
        .unwrap_or_else(|| vec!["src/test/java".to_owned()])
        .into_iter()
        .map(|value| project_root.join(value))
        .collect();
    let inherited_toolchain = inherited.as_ref().and_then(|ctx| ctx.toolchain.clone());
    let inherited_group = inherited.as_ref().and_then(|ctx| ctx.group.clone());
    let module_name = inherited.and_then(|ctx| ctx.module_name);
    let path_dependencies = extract_path_dependencies(config.dependencies.clone(), &project_root)?;

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
        group: project.group.or(inherited_group),
        module_name,
        main_class: project.main_class,
        source_dirs,
        test_source_dirs,
        resource_dir: project_root.join("src/main/resources"),
        dependencies: extract_dependency_coordinates(config.dependencies)?,
        path_dependencies,
        test_dependencies: extract_dependency_coordinates(config.test_dependencies)?,
        toolchain: parse_toolchain_request(config.toolchains).or(inherited_toolchain),
    })
}

fn inherited_workspace_context(start: &Path) -> Result<Option<WorkspaceInheritance>, ConfigError> {
    let Some(path) = find_workspace_root_jot_toml(start)? else {
        return Ok(None);
    };

    let content = fs::read_to_string(path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let workspace = config.workspace;
    Ok(Some(WorkspaceInheritance {
        group: workspace.and_then(|ws| ws.group),
        toolchain: parse_toolchain_request(config.toolchains),
        module_name: None,
    }))
}

fn module_name_from_member(member: &str) -> Result<String, ConfigError> {
    Path::new(member)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ConfigError::InvalidWorkspaceMember(member.to_owned()))
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

fn extract_path_dependencies(
    dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    project_root: &Path,
) -> Result<Vec<PathBuf>, ConfigError> {
    let mut result = Vec::new();

    for (name, spec) in dependencies.unwrap_or_default() {
        if let RawDependencySpec::Detailed {
            path: Some(path), ..
        } = spec
        {
            let candidate = project_root.join(path);
            let canonical = if candidate.exists() {
                candidate.canonicalize()?
            } else {
                return Err(ConfigError::InvalidPathDependency {
                    name,
                    path: candidate,
                    reason: "dependency path does not exist".to_owned(),
                });
            };

            if !canonical.is_dir() {
                return Err(ConfigError::InvalidPathDependency {
                    name,
                    path: canonical,
                    reason: "dependency path must point to a directory".to_owned(),
                });
            }

            if !canonical.join("jot.toml").is_file() {
                return Err(ConfigError::InvalidPathDependency {
                    name,
                    path: canonical,
                    reason: "dependency directory must contain jot.toml".to_owned(),
                });
            }

            result.push(canonical);
        }
    }

    result.sort();
    result.dedup();
    Ok(result)
}

fn detect_workspace_path_cycles(
    members: &[WorkspaceMemberBuildConfig],
) -> Result<(), ConfigError> {
    let mut by_root = std::collections::BTreeMap::new();
    for member in members {
        by_root.insert(member.project.project_root.clone(), member.module_name.clone());
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Visiting,
        Done,
    }

    fn visit(
        module: &str,
        graph: &std::collections::BTreeMap<String, Vec<String>>,
        marks: &mut std::collections::BTreeMap<String, Mark>,
        stack: &mut Vec<String>,
    ) -> Result<(), ConfigError> {
        match marks.get(module).copied() {
            Some(Mark::Done) => return Ok(()),
            Some(Mark::Visiting) => {
                stack.push(module.to_owned());
                return Err(ConfigError::WorkspacePathDependencyCycle(stack.join(" -> ")));
            }
            None => {}
        }

        marks.insert(module.to_owned(), Mark::Visiting);
        stack.push(module.to_owned());

        if let Some(neighbors) = graph.get(module) {
            for next in neighbors {
                visit(next, graph, marks, stack)?;
            }
        }

        stack.pop();
        marks.insert(module.to_owned(), Mark::Done);
        Ok(())
    }

    let mut graph = std::collections::BTreeMap::<String, Vec<String>>::new();
    for member in members {
        let deps = member
            .project
            .path_dependencies
            .iter()
            .filter_map(|path| by_root.get(path).cloned())
            .collect::<Vec<_>>();
        graph.insert(member.module_name.clone(), deps);
    }

    let mut marks = std::collections::BTreeMap::<String, Mark>::new();
    for member in members {
        let mut stack = Vec::new();
        visit(&member.module_name, &graph, &mut marks, &mut stack)?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct WorkspaceInheritance {
    group: Option<String>,
    toolchain: Option<JavaToolchainRequest>,
    module_name: Option<String>,
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
    #[error("missing [workspace] section in {0}")]
    MissingWorkspaceSection(PathBuf),
    #[error("invalid [workspace] config in {path}: {reason}")]
    InvalidWorkspaceConfig { path: PathBuf, reason: String },
    #[error("invalid workspace member path `{0}`")]
    InvalidWorkspaceMember(String),
    #[error("workspace member config not found: {0}")]
    WorkspaceMemberNotFound(PathBuf),
    #[error("duplicate workspace module name `{0}`")]
    DuplicateWorkspaceModule(String),
    #[error("invalid path dependency `{name}` at {path}: {reason}")]
    InvalidPathDependency {
        name: String,
        path: PathBuf,
        reason: String,
    },
    #[error("workspace module `{module}` depends on path outside workspace: {dependency}")]
    PathDependencyOutsideWorkspace { module: String, dependency: PathBuf },
    #[error("workspace path dependency cycle detected: {0}")]
    WorkspacePathDependencyCycle(String),
}

#[cfg(test)]
mod tests {
    use super::{
        JavaToolchainRequest, find_jot_toml, find_workspace_jot_toml,
        find_workspace_root_jot_toml, load_project_build_config, load_workspace_build_config,
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
    fn finds_nearest_workspace_root_with_workspace_section() {
        let temp = tempdir().expect("tempdir");
        let outer = temp.path().join("outer");
        let workspace = outer.join("workspace");
        let member = workspace.join("member");
        let nested = member.join("src");
        fs::create_dir_all(&nested).expect("create dirs");

        fs::write(outer.join("jot.toml"), "[project]\nname = \"outer\"\n")
            .expect("write outer config");
        fs::write(
            workspace.join("jot.toml"),
            "[workspace]\nmembers = [\"member\"]\n[toolchains]\njava = \"21\"\n",
        )
        .expect("write workspace config");

        let path = find_workspace_root_jot_toml(&nested).expect("find workspace root");
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
        assert_eq!(config.group, None);
        assert_eq!(config.module_name, None);
        assert_eq!(config.main_class.as_deref(), Some("dev.demo.Main"));
        assert_eq!(config.source_dirs, vec![temp.path().join("src/main/java")]);
        assert_eq!(config.test_source_dirs, vec![temp.path().join("src/test/java")]);
        assert_eq!(config.resource_dir, temp.path().join("src/main/resources"));
        assert_eq!(config.dependencies, vec!["org.slf4j:slf4j-api:2.0.16".to_owned()]);
        assert_eq!(config.path_dependencies, Vec::<std::path::PathBuf>::new());
        assert_eq!(config.test_dependencies, Vec::<String>::new());
        assert_eq!(config.toolchain.expect("toolchain").version, "21");
    }

    #[test]
    fn inherits_workspace_toolchain_for_member_project() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let member = workspace.join("domain");
        fs::create_dir_all(&member).expect("create dirs");

        fs::write(
            workspace.join("jot.toml"),
            "[workspace]\nmembers = [\"domain\"]\ngroup = \"com.shopflow\"\n\n[toolchains]\njava = \"21\"\n",
        )
        .expect("write workspace config");
        fs::write(
            member.join("jot.toml"),
            "[project]\nname = \"domain\"\nversion = \"1.0.0\"\n",
        )
        .expect("write member config");

        let config = load_project_build_config(&member).expect("load member config");
        assert_eq!(config.toolchain.expect("toolchain").version, "21");
        assert_eq!(config.group.as_deref(), Some("com.shopflow"));
    }

    #[test]
    fn loads_workspace_and_member_path_dependencies() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let domain = workspace.join("domain");
        let api = workspace.join("api");
        fs::create_dir_all(&domain).expect("create domain");
        fs::create_dir_all(&api).expect("create api");

        fs::write(
            workspace.join("jot.toml"),
            "[workspace]\nmembers = [\"domain\", \"api\"]\n\n[toolchains]\njava = \"21\"\n",
        )
        .expect("write workspace config");
        fs::write(
            domain.join("jot.toml"),
            "[project]\nname = \"domain\"\nversion = \"1.0.0\"\n",
        )
        .expect("write domain config");
        fs::write(
            api.join("jot.toml"),
            "[project]\nname = \"api\"\nversion = \"1.0.0\"\n\n[dependencies]\ndomain = { path = \"../domain\" }\n",
        )
        .expect("write api config");

        let workspace_config = load_workspace_build_config(&workspace)
            .expect("load workspace")
            .expect("workspace should exist");
        assert_eq!(workspace_config.members.len(), 2);

        let api_member = workspace_config
            .members
            .iter()
            .find(|member| member.module_name == "api")
            .expect("api member");
        assert_eq!(api_member.project.path_dependencies, vec![domain.canonicalize().expect("canonical domain")]);
    }
}