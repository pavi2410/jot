use std::fs;
use std::path::{Path, PathBuf};

use toml::Value as TomlValue;

use crate::errors::ConfigError;
use crate::models::WorkspaceMemberBuildConfig;
use crate::raw::{RawCatalog, RawCatalogVersion, RawConfig, RawDependencySpec, RawProcessorSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredDependencyEntry {
    pub name: String,
    pub coordinate: String,
    pub test: bool,
}

pub fn read_declared_dependencies(start: &Path) -> Result<Vec<String>, ConfigError> {
    let Some(path) = crate::discovery::find_jot_toml(start)? else {
        return Ok(Vec::new());
    };

    let content = fs::read_to_string(&path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let inherited = crate::inherited_workspace_context(path.parent().unwrap_or(start))?;
    let catalog_path = inherited
        .and_then(|ctx| ctx.catalog_path)
        .or_else(|| path.parent().and_then(catalog_path_for_root));
    let mut coords = extract_dependency_coordinates(config.dependencies, catalog_path.as_deref())?;
    coords.extend(extract_dependency_coordinates(
        config.test_dependencies,
        catalog_path.as_deref(),
    )?);
    Ok(coords)
}

pub fn read_declared_dependency_entries(
    start: &Path,
) -> Result<Vec<DeclaredDependencyEntry>, ConfigError> {
    let Some(path) = crate::discovery::find_jot_toml(start)? else {
        return Ok(Vec::new());
    };

    let content = fs::read_to_string(&path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let inherited = crate::inherited_workspace_context(path.parent().unwrap_or(start))?;
    let catalog_path = inherited
        .and_then(|ctx| ctx.catalog_path)
        .or_else(|| path.parent().and_then(catalog_path_for_root));
    let catalog = load_catalog(catalog_path.as_deref())?;

    let mut entries = Vec::new();
    entries.extend(resolve_dependency_entries(
        config.dependencies,
        false,
        catalog.as_ref(),
    )?);
    entries.extend(resolve_dependency_entries(
        config.test_dependencies,
        true,
        catalog.as_ref(),
    )?);
    entries.sort_by(|left, right| {
        left.test
            .cmp(&right.test)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}

fn resolve_dependency_entries(
    dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    test: bool,
    catalog: Option<&RawCatalog>,
) -> Result<Vec<DeclaredDependencyEntry>, ConfigError> {
    let mut result = Vec::new();

    for (name, spec) in dependencies.unwrap_or_default() {
        match spec {
            RawDependencySpec::Coords(coords) => {
                result.push(DeclaredDependencyEntry {
                    name,
                    coordinate: coords,
                    test,
                });
            }
            RawDependencySpec::Detailed {
                coords: Some(coords),
                ..
            } => {
                result.push(DeclaredDependencyEntry {
                    name,
                    coordinate: coords,
                    test,
                });
            }
            RawDependencySpec::Detailed { path: Some(_), .. } => {}
            RawDependencySpec::Detailed {
                catalog: Some(alias),
                ..
            } => {
                result.push(DeclaredDependencyEntry {
                    coordinate: resolve_catalog_dependency(&name, &alias, catalog)?,
                    name,
                    test,
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

pub(crate) fn extract_dependency_coordinates(
    dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    catalog_path: Option<&Path>,
) -> Result<Vec<String>, ConfigError> {
    let mut result = Vec::new();
    let catalog = load_catalog(catalog_path)?;

    for (name, spec) in dependencies.unwrap_or_default() {
        match spec {
            RawDependencySpec::Coords(coords) => result.push(coords),
            RawDependencySpec::Detailed {
                coords: Some(coords),
                ..
            } => result.push(coords),
            RawDependencySpec::Detailed { path: Some(_), .. } => {}
            RawDependencySpec::Detailed {
                catalog: Some(alias),
                ..
            } => {
                result.push(resolve_catalog_dependency(&name, &alias, catalog.as_ref())?);
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

pub(crate) fn catalog_path_for_root(root: &Path) -> Option<PathBuf> {
    let path = root.join("libs.versions.toml");
    path.is_file().then_some(path)
}

fn load_catalog(catalog_path: Option<&Path>) -> Result<Option<RawCatalog>, ConfigError> {
    let Some(path) = catalog_path else {
        return Ok(None);
    };

    let content = fs::read_to_string(path)?;
    let value = content.parse::<TomlValue>()?;
    let catalog = value.try_into::<RawCatalog>()?;
    Ok(Some(catalog))
}

fn resolve_catalog_dependency(
    dependency_name: &str,
    alias: &str,
    catalog: Option<&RawCatalog>,
) -> Result<String, ConfigError> {
    let catalog = catalog.ok_or_else(|| ConfigError::MissingCatalogFile {
        dependency: dependency_name.to_owned(),
    })?;
    let library = catalog
        .libraries
        .as_ref()
        .and_then(|libraries| libraries.get(alias))
        .ok_or_else(|| ConfigError::MissingCatalogEntry {
            dependency: dependency_name.to_owned(),
            alias: alias.to_owned(),
        })?;

    let version = match library.version.as_ref() {
        Some(RawCatalogVersion::Literal(version)) => Some(version.clone()),
        Some(RawCatalogVersion::Detailed { r#ref }) => Some(
            catalog
                .versions
                .as_ref()
                .and_then(|versions| versions.get(r#ref))
                .cloned()
                .ok_or_else(|| ConfigError::MissingCatalogVersion {
                    dependency: dependency_name.to_owned(),
                    alias: alias.to_owned(),
                    version_ref: r#ref.clone(),
                })?,
        ),
        None => None,
    };

    Ok(match version {
        Some(version) => format!("{}:{}", library.module, version),
        None => library.module.clone(),
    })
}

pub(crate) fn extract_path_dependencies(
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

pub(crate) fn detect_workspace_path_cycles(
    members: &[WorkspaceMemberBuildConfig],
) -> Result<(), ConfigError> {
    let mut by_root = std::collections::BTreeMap::new();
    for member in members {
        by_root.insert(
            member.project.project_root.clone(),
            member.module_name.clone(),
        );
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
                return Err(ConfigError::WorkspacePathDependencyCycle(
                    stack.join(" -> "),
                ));
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

pub(crate) fn extract_processor_specs(
    processors: Option<std::collections::BTreeMap<String, RawProcessorSpec>>,
    catalog_path: Option<&Path>,
) -> Result<(Vec<String>, std::collections::BTreeMap<String, String>), ConfigError> {
    let mut coordinates = Vec::new();
    let mut options = std::collections::BTreeMap::new();
    let catalog = load_catalog(catalog_path)?;

    for (name, spec) in processors.unwrap_or_default() {
        match spec {
            RawProcessorSpec::Coords(coords) => coordinates.push(coords),
            RawProcessorSpec::Detailed {
                coords: Some(coords),
                options: proc_options,
                ..
            } => {
                coordinates.push(coords);
                if let Some(proc_options) = proc_options {
                    options.extend(proc_options);
                }
            }
            RawProcessorSpec::Detailed {
                catalog: Some(alias),
                options: proc_options,
                ..
            } => {
                coordinates.push(resolve_catalog_dependency(&name, &alias, catalog.as_ref())?);
                if let Some(proc_options) = proc_options {
                    options.extend(proc_options);
                }
            }
            RawProcessorSpec::Detailed { .. } => {
                return Err(ConfigError::UnsupportedDependencyDeclaration {
                    name,
                    reason: "processor declaration must include `coords` or `catalog`".to_owned(),
                });
            }
        }
    }

    Ok((coordinates, options))
}

pub(crate) fn module_name_from_member(member: &str) -> Result<String, ConfigError> {
    Path::new(member)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ConfigError::InvalidWorkspaceMember(member.to_owned()))
}
