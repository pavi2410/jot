use std::path::PathBuf;

use jot_cache::JotPaths;
use jot_config::{
    DependencySpec, add_dependency, load_workspace_dependency_set, read_declared_dependencies,
    remove_dependency,
};
use jot_resolver::{MavenCoordinate, MavenResolver, TreeEntry};

use crate::utils::nearest_project_file;
use crate::utils::write_locked_file;

const DEFAULT_LOCK_DEPTH: usize = 8;
const DEFAULT_LOCKFILE_NAME: &str = "jot.lock";

pub(crate) fn handle_lock(
    dependencies: &[String],
    depth: usize,
    output: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let workspace_dependencies = load_workspace_dependency_set(&cwd)?;
    let resolved_inputs = if dependencies.is_empty() {
        let inputs = if let Some(workspace) = workspace_dependencies.as_ref() {
            workspace.external_dependencies.clone()
        } else {
            read_declared_dependencies(&cwd)?
        };
        if inputs.is_empty() {
            return Err(
                "no dependency coordinates were provided and no supported `[dependencies]` entries were found in jot.toml"
                    .into(),
            );
        }
        inputs
    } else {
        dependencies.to_vec()
    };

    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let resolver = MavenResolver::new(paths.clone())?;
    let lockfile = resolver.resolve_lockfile(&resolved_inputs, depth)?;
    let content = toml::to_string_pretty(&lockfile)?;
    let output_path = if dependencies.is_empty() && output == &PathBuf::from("jot.lock") {
        workspace_dependencies
            .as_ref()
            .map(|workspace| workspace.root_dir.join("jot.lock"))
            .unwrap_or_else(|| output.clone())
    } else {
        output.clone()
    };
    write_locked_file(&paths, &output_path, content.as_bytes())?;
    println!("wrote {}", output_path.display());
    Ok(())
}

pub(crate) fn handle_resolve(
    dependency: &str,
    deps: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let resolver = MavenResolver::new(paths)?;
    if deps {
        let (coordinate, dependencies) = resolver.resolve_direct_dependencies(dependency)?;
        println!("{}", coordinate);
        if dependencies.is_empty() {
            println!("  (no direct dependencies)");
        } else {
            for dependency in dependencies {
                let version = dependency.version.unwrap_or_else(|| "<managed>".to_owned());
                let scope = dependency.scope.unwrap_or_else(|| "compile".to_owned());
                let optional = if dependency.optional { " optional" } else { "" };
                println!(
                    "  - {}:{}:{} [{}{}]",
                    dependency.group, dependency.artifact, version, scope, optional
                );
            }
        }
    } else {
        let coordinate = resolver.resolve_coordinate(dependency)?;
        println!("{}", coordinate);
    }
    Ok(())
}

pub(crate) fn handle_tree(
    dependency: Option<&str>,
    depth: usize,
    workspace: bool,
    module: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let resolver = MavenResolver::new(paths)?;

    if workspace {
        if dependency.is_some() {
            return Err("dependency argument cannot be combined with --workspace".into());
        }
        return print_workspace_tree(&resolver, &std::env::current_dir()?, depth, module);
    }

    let dependency = dependency.ok_or("tree requires a dependency coordinate or --workspace")?;
    let entries = resolver.resolve_dependency_tree(dependency, depth)?;
    for entry in entries {
        print_tree_entry(&entry, 0);
    }
    Ok(())
}

pub(crate) fn handle_add(
    coordinate: Option<&str>,
    catalog: Option<&str>,
    test: bool,
    name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let project_file = nearest_project_file(&cwd)?;

    let (dependency_name, spec) = resolve_add_input(coordinate, catalog, name)?;
    add_dependency(&project_file, &dependency_name, spec, test)?;

    println!(
        "added dependency `{}` to {} [{}]",
        dependency_name,
        project_file.display(),
        if test {
            "test-dependencies"
        } else {
            "dependencies"
        }
    );

    regenerate_lockfile_if_possible()
}

pub(crate) fn handle_remove(name: &str, test: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let project_file = nearest_project_file(&cwd)?;

    let removed = remove_dependency(&project_file, name, test)?;
    if !removed {
        return Err(format!(
            "dependency `{name}` was not found in [{}]",
            if test {
                "test-dependencies"
            } else {
                "dependencies"
            }
        )
        .into());
    }

    println!(
        "removed dependency `{}` from {} [{}]",
        name,
        project_file.display(),
        if test {
            "test-dependencies"
        } else {
            "dependencies"
        }
    );

    regenerate_lockfile_if_possible()
}

fn resolve_add_input(
    coordinate: Option<&str>,
    catalog: Option<&str>,
    name: Option<&str>,
) -> Result<(String, DependencySpec), Box<dyn std::error::Error>> {
    match (coordinate, catalog) {
        (Some(_), Some(_)) => {
            Err("use either a coordinate argument or --catalog, but not both".into())
        }
        (None, None) => Err("missing dependency input: provide <group:artifact:version> or --catalog <name>".into()),
        (Some(raw), None) => {
            let parsed = MavenCoordinate::parse(raw)?;
            if parsed.version.is_none() {
                return Err("coordinate must include a version: <group:artifact:version>".into());
            }

            let dependency_name = name
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| parsed.artifact.clone());
            Ok((dependency_name, DependencySpec::Coords(parsed.to_string())))
        }
        (None, Some(alias)) => {
            let dependency_name = name.map(ToOwned::to_owned).unwrap_or_else(|| alias.to_owned());
            Ok((dependency_name, DependencySpec::Catalog(alias.to_owned())))
        }
    }
}

fn regenerate_lockfile_if_possible() -> Result<(), Box<dyn std::error::Error>> {
    let lock_output = PathBuf::from(DEFAULT_LOCKFILE_NAME);
    match handle_lock(&[], DEFAULT_LOCK_DEPTH, &lock_output) {
        Ok(()) => Ok(()),
        Err(error) => {
            if error
                .to_string()
                .contains("no dependency coordinates were provided")
            {
                println!("skipped lockfile regeneration: no declared external dependencies");
                return Ok(());
            }
            Err(error)
        }
    }
}

fn print_tree_entry(entry: &TreeEntry, base_depth: usize) {
    let indent = "  ".repeat(entry.depth + base_depth);
    let scope = entry.scope.clone().unwrap_or_else(|| "compile".to_owned());
    let optional = if entry.optional { " optional" } else { "" };
    let note = entry
        .note
        .as_ref()
        .map(|value| format!(" ({value})"))
        .unwrap_or_default();

    if entry.depth == 0 {
        if base_depth == 0 {
            println!("{}", entry.coordinate);
        } else {
            println!("{}- {}", indent, entry.coordinate);
        }
        return;
    }

    println!(
        "{}- {} [{}{}]{}",
        indent, entry.coordinate, scope, optional, note
    );
}

fn print_workspace_tree(
    resolver: &MavenResolver,
    start: &std::path::Path,
    depth: usize,
    module: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = load_workspace_dependency_set(start)?
        .ok_or("--workspace requires running inside a workspace")?;
    if let Some(selected) = module
        && !workspace
            .members
            .iter()
            .any(|member| member.module_name == selected)
    {
        return Err(format!("unknown workspace module `{selected}`").into());
    }
    let by_root = workspace
        .members
        .iter()
        .map(|member| (member.project_root.clone(), member.module_name.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();

    println!("workspace");
    for member in workspace.members {
        if module.is_some_and(|selected| selected != member.module_name) {
            continue;
        }

        println!("- {}", member.module_name);
        for path_dependency in &member.path_dependencies {
            let dependency_name = by_root
                .get(path_dependency)
                .cloned()
                .unwrap_or_else(|| path_dependency.display().to_string());
            println!("  - {} (workspace)", dependency_name);
        }

        for dependency in &member.external_dependencies {
            let entries = resolver.resolve_dependency_tree(dependency, depth)?;
            for entry in entries {
                print_tree_entry(&entry, 1);
            }
        }
    }

    Ok(())
}
