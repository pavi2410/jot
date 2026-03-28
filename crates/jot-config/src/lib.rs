mod dependencies;
mod discovery;
mod editing;
mod errors;
mod models;
mod raw;

pub use dependencies::{
    DeclaredDependencyEntry, DependencyScope, read_declared_dependencies,
    read_declared_dependency_entries,
};
pub use discovery::{
    find_jot_toml, find_workspace_jot_toml, find_workspace_root_jot_toml, read_toolchain_request,
};
pub use editing::{DependencySpec, add_dependency, pin_java_toolchain, remove_dependency};
pub use errors::ConfigError;
pub use models::{
    BenchConfig, FormatConfig, JavaFormatStyle, LintConfig, ProjectBuildConfig, PublishConfig,
    PublishDeveloper, WorkspaceBuildConfig, WorkspaceDependencySet, WorkspaceMemberBuildConfig,
    WorkspaceMemberDependencies,
};

use std::fs;
use std::path::Path;

use jot_toolchain::{JavaToolchainRequest, KotlinToolchainRequest};

use crate::dependencies::{
    catalog_path_for_root, detect_workspace_path_cycles, extract_dependency_coordinates,
    extract_path_dependencies, extract_processor_specs, module_name_from_member,
};
use crate::models::WorkspaceInheritance;
use crate::raw::{
    RawBench, RawConfig, RawFormat, RawJavaToolchain, RawLint, RawPublish, RawPublishDeveloper,
    RawToolchains,
};

pub fn load_project_build_config(start: &Path) -> Result<ProjectBuildConfig, ConfigError> {
    let Some(config_path) = find_jot_toml(start)? else {
        return Err(ConfigError::ProjectConfigNotFound(start.to_path_buf()));
    };

    load_project_build_config_from_file(&config_path)
}

pub fn load_workspace_build_config(
    start: &Path,
) -> Result<Option<WorkspaceBuildConfig>, ConfigError> {
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

    let root_toolchain = parse_toolchain_request(root_config.toolchains.as_ref());
    let root_kotlin_toolchain = parse_kotlin_toolchain_request(root_config.toolchains.as_ref());
    let mut members = Vec::new();
    let mut seen_names = std::collections::BTreeSet::new();

    for member in workspace.members.iter() {
        let member_root = root_dir.join(member);
        let member_config_path = member_root.join("jot.toml");
        if !member_config_path.is_file() {
            return Err(ConfigError::WorkspaceMemberNotFound(member_config_path));
        }

        let module_name = module_name_from_member(member)?;
        if !seen_names.insert(module_name.clone()) {
            return Err(ConfigError::DuplicateWorkspaceModule(module_name));
        }

        let inherited = WorkspaceInheritance {
            group: workspace.group.clone(),
            toolchain: root_toolchain.clone(),
            kotlin_toolchain: root_kotlin_toolchain.clone(),
            module_name: Some(module_name.clone()),
            catalog_path: catalog_path_for_root(&root_dir),
            publish: parse_publish_config(root_config.publish.clone(), None),
            format: Some(parse_format_config(root_config.format.clone(), None)),
            lint: Some(parse_lint_config(root_config.lint.clone(), None, &root_dir)),
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
        kotlin_toolchain: root_kotlin_toolchain,
        members,
    }))
}

pub fn load_workspace_dependency_set(
    start: &Path,
) -> Result<Option<WorkspaceDependencySet>, ConfigError> {
    let Some(workspace) = load_workspace_build_config(start)? else {
        return Ok(None);
    };

    let mut external_dependencies = workspace
        .members
        .iter()
        .flat_map(|member| {
            member
                .project
                .dependencies
                .iter()
                .chain(member.project.test_dependencies.iter())
                .cloned()
        })
        .collect::<Vec<_>>();
    external_dependencies.sort();
    external_dependencies.dedup();

    Ok(Some(WorkspaceDependencySet {
        root_dir: workspace.root_dir.clone(),
        members: workspace
            .members
            .into_iter()
            .map(|member| WorkspaceMemberDependencies {
                module_name: member.module_name,
                project_root: member.project.project_root,
                path_dependencies: member.project.path_dependencies,
                external_dependencies: member.project.dependencies,
            })
            .collect(),
        external_dependencies,
    }))
}

fn load_project_build_config_from_file(
    config_path: &Path,
) -> Result<ProjectBuildConfig, ConfigError> {
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
    let inherited_toolchain = inherited.as_ref().and_then(|ctx| ctx.toolchain.clone());
    let inherited_kotlin_toolchain = inherited
        .as_ref()
        .and_then(|ctx| ctx.kotlin_toolchain.clone());
    let kotlin_toolchain =
        parse_kotlin_toolchain_request(config.toolchains.as_ref()).or(inherited_kotlin_toolchain);
    let has_kotlin = kotlin_toolchain.is_some();
    let source_dirs = project
        .source_dirs
        .unwrap_or_else(|| {
            let mut dirs = vec!["src/main/java".to_owned()];
            if has_kotlin {
                dirs.push("src/main/kotlin".to_owned());
            }
            dirs
        })
        .into_iter()
        .map(|value| project_root.join(value))
        .collect();
    let test_source_dirs = project
        .test_source_dirs
        .unwrap_or_else(|| {
            let mut dirs = vec!["src/test/java".to_owned()];
            if has_kotlin {
                dirs.push("src/test/kotlin".to_owned());
            }
            dirs
        })
        .into_iter()
        .map(|value| project_root.join(value))
        .collect();
    let inherited_group = inherited.as_ref().and_then(|ctx| ctx.group.clone());
    let inherited_catalog_path = inherited.as_ref().and_then(|ctx| ctx.catalog_path.clone());
    let inherited_publish = inherited.as_ref().and_then(|ctx| ctx.publish.clone());
    let inherited_format = inherited.as_ref().and_then(|ctx| ctx.format.clone());
    let inherited_lint = inherited.as_ref().and_then(|ctx| ctx.lint.clone());
    let module_name = inherited.and_then(|ctx| ctx.module_name);
    let path_dependencies = extract_path_dependencies(config.dependencies.clone(), &project_root)?;
    let catalog_path = inherited_catalog_path.or_else(|| catalog_path_for_root(&project_root));
    let (processors, processor_options) =
        extract_processor_specs(config.processors, catalog_path.as_deref())?;

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
        dependencies: extract_dependency_coordinates(config.dependencies, catalog_path.as_deref())?,
        path_dependencies,
        test_dependencies: extract_dependency_coordinates(
            config.test_dependencies,
            catalog_path.as_deref(),
        )?,
        processors,
        processor_options,
        toolchain: parse_toolchain_request(config.toolchains.as_ref()).or(inherited_toolchain),
        kotlin_toolchain,
        publish: parse_publish_config(config.publish, inherited_publish),
        format: parse_format_config(config.format, inherited_format),
        lint: parse_lint_config(config.lint, inherited_lint, &project_root),
        bench: parse_bench_config(config.bench, has_kotlin, &project_root),
    })
}

fn inherited_workspace_context(start: &Path) -> Result<Option<WorkspaceInheritance>, ConfigError> {
    let Some(path) = find_workspace_root_jot_toml(start)? else {
        return Ok(None);
    };

    let content = fs::read_to_string(&path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let workspace = config.workspace;
    Ok(Some(WorkspaceInheritance {
        group: workspace.and_then(|ws| ws.group),
        toolchain: parse_toolchain_request(config.toolchains.as_ref()),
        kotlin_toolchain: parse_kotlin_toolchain_request(config.toolchains.as_ref()),
        module_name: None,
        catalog_path: path.parent().and_then(catalog_path_for_root),
        publish: parse_publish_config(config.publish, None),
        format: Some(parse_format_config(config.format, None)),
        lint: Some(parse_lint_config(
            config.lint,
            None,
            path.parent().unwrap_or(start),
        )),
    }))
}

fn parse_format_config(raw: Option<RawFormat>, inherited: Option<FormatConfig>) -> FormatConfig {
    let mut config = inherited.unwrap_or_default();
    if let Some(raw) = raw
        && let Some(java_style) = raw.java_style
    {
        config.java_style = java_style;
    }
    config
}

fn parse_lint_config(
    raw: Option<RawLint>,
    inherited: Option<LintConfig>,
    config_root: &Path,
) -> LintConfig {
    let mut config = inherited.unwrap_or_default();
    if let Some(raw) = raw
        && let Some(pmd_ruleset) = raw.pmd_ruleset
    {
        config.pmd_ruleset = Some(config_root.join(pmd_ruleset));
    }
    config
}

fn parse_bench_config(raw: Option<RawBench>, has_kotlin: bool, root: &Path) -> BenchConfig {
    let raw = raw.unwrap_or_default();
    let source_dirs = raw
        .source_dirs
        .unwrap_or_else(|| {
            let mut dirs = vec!["src/bench/java".to_owned()];
            if has_kotlin {
                dirs.push("src/bench/kotlin".to_owned());
            }
            dirs
        })
        .into_iter()
        .map(|v| root.join(v))
        .collect();
    BenchConfig {
        jmh_version: raw.jmh_version,
        source_dirs,
        deps: raw.deps.unwrap_or_default(),
    }
}

fn parse_publish_config(
    raw: Option<RawPublish>,
    inherited: Option<PublishConfig>,
) -> Option<PublishConfig> {
    let mut config = inherited.unwrap_or_default();
    let mut touched = false;

    if config != PublishConfig::default() {
        touched = true;
    }

    if let Some(raw) = raw {
        if let Some(license) = raw.license {
            config.license = Some(license);
            touched = true;
        }
        if let Some(description) = raw.description {
            config.description = Some(description);
            touched = true;
        }
        if let Some(url) = raw.url {
            config.url = Some(url);
            touched = true;
        }
        if let Some(scm) = raw.scm {
            config.scm = Some(scm);
            touched = true;
        }
        if let Some(developer) = raw.developer
            && let Some(developer) = parse_publish_developer(developer)
        {
            config.developer = Some(developer);
            touched = true;
        }
    }

    touched.then_some(config)
}

fn parse_publish_developer(raw: RawPublishDeveloper) -> Option<PublishDeveloper> {
    let name = raw.name?.trim().to_owned();
    if name.is_empty() {
        return None;
    }

    Some(PublishDeveloper {
        name,
        email: raw
            .email
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    })
}

fn parse_toolchain_request(toolchains: Option<&RawToolchains>) -> Option<JavaToolchainRequest> {
    let toolchains = toolchains?;
    toolchains.java.as_ref().map(|java| match java {
        RawJavaToolchain::Version(version) => JavaToolchainRequest {
            version: version.clone(),
            vendor: None,
        },
        RawJavaToolchain::Detailed { version, vendor } => JavaToolchainRequest {
            version: version.clone(),
            vendor: *vendor,
        },
    })
}

fn parse_kotlin_toolchain_request(
    toolchains: Option<&RawToolchains>,
) -> Option<KotlinToolchainRequest> {
    let toolchains = toolchains?;
    toolchains
        .kotlin
        .as_ref()
        .map(|version| KotlinToolchainRequest {
            version: version.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        DependencySpec, JavaToolchainRequest, add_dependency, find_jot_toml,
        find_workspace_jot_toml, find_workspace_root_jot_toml, load_project_build_config,
        load_workspace_build_config, load_workspace_dependency_set, pin_java_toolchain,
        read_declared_dependencies, read_toolchain_request, remove_dependency,
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
        fs::write(
            workspace.join("jot.toml"),
            "[workspace]\nmembers = [\"member\"]\n",
        )
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
    fn inherits_publish_metadata_from_workspace_root() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let member = workspace.join("lib");
        fs::create_dir_all(&member).expect("create member");

        fs::write(
            workspace.join("jot.toml"),
            "[workspace]\nmembers = [\"lib\"]\ngroup = \"io.github.demo\"\n\n[publish]\nlicense = \"Apache-2.0\"\ndescription = \"workspace package\"\nurl = \"https://example.com\"\nscm = \"https://example.com/repo.git\"\ndeveloper = { name = \"Pavi\", email = \"pavi@example.com\" }\n",
        )
        .expect("write workspace config");
        fs::write(
            member.join("jot.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write member config");

        let workspace = load_workspace_build_config(&member)
            .expect("load workspace")
            .expect("workspace config");
        let publish = workspace.members[0]
            .project
            .publish
            .clone()
            .expect("publish config");

        assert_eq!(publish.license.as_deref(), Some("Apache-2.0"));
        assert_eq!(publish.description.as_deref(), Some("workspace package"));
        assert_eq!(publish.url.as_deref(), Some("https://example.com"));
        assert_eq!(publish.scm.as_deref(), Some("https://example.com/repo.git"));
        assert_eq!(publish.developer.expect("developer").name, "Pavi");
    }

    #[test]
    fn adds_and_removes_main_dependency_entry() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(
            &config_path,
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");

        add_dependency(
            &config_path,
            "slf4j",
            DependencySpec::Coords("org.slf4j:slf4j-api:2.0.16".to_owned()),
            false,
        )
        .expect("add dependency");

        let content = fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("[dependencies]"));
        assert!(content.contains("slf4j = \"org.slf4j:slf4j-api:2.0.16\""));

        let removed = remove_dependency(&config_path, "slf4j", false).expect("remove dependency");
        assert!(removed);

        let content = fs::read_to_string(&config_path).expect("read config");
        assert!(!content.contains("slf4j = \"org.slf4j:slf4j-api:2.0.16\""));
    }

    #[test]
    fn adds_test_catalog_dependency_entry() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(
            &config_path,
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");

        add_dependency(
            &config_path,
            "junit",
            DependencySpec::Catalog("junit".to_owned()),
            true,
        )
        .expect("add test dependency");

        let content = fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("[test-dependencies]"));
        assert!(content.contains("junit = { catalog = \"junit\" }"));
    }

    #[test]
    fn resolves_catalog_dependencies_from_project_root_catalog() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(
            temp.path().join("libs.versions.toml"),
            "[versions]\njunit = \"5.11.0\"\n\n[libraries]\njunit = { module = \"org.junit.jupiter:junit-jupiter\", version.ref = \"junit\" }\n",
        )
        .expect("write catalog");
        fs::write(
            &config_path,
            "[dependencies]\njunit = { catalog = \"junit\" }\n",
        )
        .expect("write config");

        let dependencies =
            read_declared_dependencies(&config_path).expect("resolve catalog dependency");
        assert_eq!(
            dependencies,
            vec!["org.junit.jupiter:junit-jupiter:5.11.0".to_owned()]
        );
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
        assert_eq!(
            config.test_source_dirs,
            vec![temp.path().join("src/test/java")]
        );
        assert_eq!(config.resource_dir, temp.path().join("src/main/resources"));
        assert_eq!(
            config.dependencies,
            vec!["org.slf4j:slf4j-api:2.0.16".to_owned()]
        );
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
        assert_eq!(
            api_member.project.path_dependencies,
            vec![domain.canonicalize().expect("canonical domain")]
        );
    }

    #[test]
    fn resolves_workspace_member_catalog_dependencies_from_root_catalog() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let api = workspace.join("api");
        fs::create_dir_all(&api).expect("create api");

        fs::write(
            workspace.join("jot.toml"),
            "[workspace]\nmembers = [\"api\"]\n\n[toolchains]\njava = \"21\"\n",
        )
        .expect("write workspace config");
        fs::write(
            workspace.join("libs.versions.toml"),
            "[versions]\npicocli = \"4.7.6\"\n\n[libraries]\npicocli = { module = \"info.picocli:picocli\", version.ref = \"picocli\" }\n",
        )
        .expect("write workspace catalog");
        fs::write(
            api.join("jot.toml"),
            "[project]\nname = \"api\"\nversion = \"1.0.0\"\n\n[dependencies]\npicocli = { catalog = \"picocli\" }\n",
        )
        .expect("write api config");

        let config = load_project_build_config(&api).expect("load member config");
        assert_eq!(
            config.dependencies,
            vec!["info.picocli:picocli:4.7.6".to_owned()]
        );
    }

    #[test]
    fn aggregates_workspace_external_dependencies_for_locking() {
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
            "[project]\nname = \"domain\"\nversion = \"1.0.0\"\n\n[dependencies]\njackson = \"com.fasterxml.jackson.core:jackson-databind:2.18.0\"\n",
        )
        .expect("write domain config");
        fs::write(
            api.join("jot.toml"),
            "[project]\nname = \"api\"\nversion = \"1.0.0\"\n\n[dependencies]\ndomain = { path = \"../domain\" }\njackson = \"com.fasterxml.jackson.core:jackson-databind:2.18.0\"\npicocli = \"info.picocli:picocli:4.7.6\"\n",
        )
        .expect("write api config");

        let dependencies = load_workspace_dependency_set(&workspace)
            .expect("load workspace dependency set")
            .expect("workspace set");

        assert_eq!(
            dependencies.external_dependencies,
            vec![
                "com.fasterxml.jackson.core:jackson-databind:2.18.0".to_owned(),
                "info.picocli:picocli:4.7.6".to_owned(),
            ]
        );
        assert_eq!(dependencies.members.len(), 2);
    }

    // ── discovery.rs tests ──────────────────────────────────────────────

    #[test]
    fn find_jot_toml_returns_none_for_empty_tree() {
        let temp = tempdir().expect("tempdir");
        let result = find_jot_toml(temp.path()).expect("find_jot_toml");
        assert!(result.is_none());
    }

    #[test]
    fn find_workspace_jot_toml_returns_topmost() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("create dirs");
        fs::write(
            root.join("jot.toml"),
            "[project]\nname = \"root\"\nversion = \"1.0.0\"\n",
        )
        .expect("write root config");
        fs::write(
            sub.join("jot.toml"),
            "[project]\nname = \"sub\"\nversion = \"1.0.0\"\n",
        )
        .expect("write sub config");

        let result = find_workspace_jot_toml(&sub).expect("find_workspace_jot_toml");
        assert_eq!(result.unwrap(), root.join("jot.toml"));
    }

    #[test]
    fn find_workspace_root_jot_toml_requires_workspace_section() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("create dirs");

        // root has no [workspace] section
        fs::write(
            root.join("jot.toml"),
            "[project]\nname = \"root\"\nversion = \"1.0.0\"\n",
        )
        .expect("write root config");
        fs::write(
            sub.join("jot.toml"),
            "[project]\nname = \"sub\"\nversion = \"1.0.0\"\n",
        )
        .expect("write sub config");

        let result = find_workspace_root_jot_toml(&sub).expect("find_workspace_root_jot_toml");
        assert!(
            result.is_none(),
            "should not find workspace without [workspace] section"
        );

        // Now add [workspace] to root
        fs::write(
            root.join("jot.toml"),
            "[workspace]\nmembers = [\"sub\"]\n\n[toolchains]\njava = \"21\"\n",
        )
        .expect("write workspace config");

        let result = find_workspace_root_jot_toml(&sub).expect("find_workspace_root_jot_toml");
        assert_eq!(result.unwrap(), root.join("jot.toml"));
    }

    // ── dependencies.rs tests ───────────────────────────────────────────

    #[test]
    fn dependency_scope_display() {
        use super::dependencies::DependencyScope;
        assert_eq!(DependencyScope::Main.to_string(), "main");
        assert_eq!(DependencyScope::Test.to_string(), "test");
    }

    #[test]
    fn dependency_scope_ordering() {
        use super::dependencies::DependencyScope;
        assert!(DependencyScope::Main < DependencyScope::Test);
    }

    #[test]
    fn read_declared_dependency_entries_returns_scoped_entries() {
        use super::{DependencyScope, read_declared_dependency_entries};

        let temp = tempdir().expect("tempdir");
        let project = temp.path().join("project");
        fs::create_dir_all(&project).expect("create project");
        fs::write(
            project.join("jot.toml"),
            r#"
[project]
name = "demo"
version = "1.0.0"

[dependencies]
guava = "com.google.guava:guava:33.0.0-jre"

[test-dependencies]
junit = "org.junit.jupiter:junit-jupiter:5.11.0"
"#,
        )
        .expect("write config");

        let entries = read_declared_dependency_entries(&project).expect("read entries");
        assert_eq!(entries.len(), 2);

        let main_entry = entries.iter().find(|e| e.name == "guava").expect("guava");
        assert_eq!(main_entry.scope, DependencyScope::Main);

        let test_entry = entries.iter().find(|e| e.name == "junit").expect("junit");
        assert_eq!(test_entry.scope, DependencyScope::Test);
    }

    #[test]
    fn cycle_detection_catches_circular_path_dependencies() {
        use super::dependencies::detect_workspace_path_cycles;
        use super::models::{
            FormatConfig, LintConfig, ProjectBuildConfig, WorkspaceMemberBuildConfig,
        };
        use std::collections::BTreeMap;
        use std::path::PathBuf;

        let make_member = |name: &str, root: &str, deps: Vec<PathBuf>| WorkspaceMemberBuildConfig {
            module_name: name.to_owned(),
            project: ProjectBuildConfig {
                config_path: PathBuf::from(root).join("jot.toml"),
                project_root: PathBuf::from(root),
                name: name.to_owned(),
                version: "1.0.0".to_owned(),
                group: None,
                module_name: None,
                main_class: None,
                source_dirs: Vec::new(),
                test_source_dirs: Vec::new(),
                resource_dir: PathBuf::from(root).join("src/main/resources"),
                dependencies: Vec::new(),
                path_dependencies: deps,
                test_dependencies: Vec::new(),
                processors: Vec::new(),
                processor_options: BTreeMap::new(),
                toolchain: None,
                kotlin_toolchain: None,
                publish: None,
                format: FormatConfig::default(),
                lint: LintConfig::default(),
                bench: Default::default(),
            },
        };

        let member_a = make_member("a", "/workspace/a", vec![PathBuf::from("/workspace/b")]);
        let member_b = make_member("b", "/workspace/b", vec![PathBuf::from("/workspace/a")]);

        let result = detect_workspace_path_cycles(&[member_a, member_b]);
        assert!(result.is_err(), "should detect a -> b -> a cycle");
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("a") && error.contains("b"),
            "error should mention both modules: {error}"
        );
    }

    #[test]
    fn module_name_from_member_extracts_last_segment() {
        use super::dependencies::module_name_from_member;
        assert_eq!(module_name_from_member("modules/api").unwrap(), "api");
        assert_eq!(module_name_from_member("core").unwrap(), "core");
        assert!(module_name_from_member("").is_err());
    }
}
