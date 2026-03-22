use jot_cache::JotPaths;
use jot_config::{
    DependencySpec, add_dependency, load_workspace_build_config, load_workspace_dependency_set,
    read_declared_dependencies, read_declared_dependency_entries, remove_dependency,
};
use jot_resolver::{LockedPackage, Lockfile, MavenCoordinate, MavenResolver, TreeEntry};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::commands::render::{format_tree_entry, print_sharp_table};
use crate::utils::nearest_project_file;
use crate::utils::write_locked_file;

const DEFAULT_LOCK_DEPTH: usize = 8;
const DEFAULT_LOCKFILE_NAME: &str = "jot.lock";

#[derive(Debug, Clone)]
struct DirectDependencyRow {
    module: Option<String>,
    name: String,
    coordinate: String,
    scope: &'static str,
}

#[derive(Debug)]
struct DepsSelection {
    rows: Vec<DirectDependencyRow>,
    lockfile_path: PathBuf,
}

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
        (None, None) => Err(
            "missing dependency input: provide <group:artifact:version> or --catalog <name>".into(),
        ),
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
            let dependency_name = name
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| alias.to_owned());
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

pub(crate) fn handle_deps(module: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let selection = collect_dependency_rows(module)?;
    let lockfile = load_lockfile(&selection.lockfile_path)?;

    if selection.rows.is_empty() {
        println!("no declared dependencies found");
        return Ok(());
    }

    let include_module_column = selection.rows.iter().any(|row| row.module.is_some());
    let mut table = Vec::with_capacity(selection.rows.len());
    for row in selection.rows {
        let resolved_version = resolve_locked_version(&lockfile, &row.coordinate)
            .unwrap_or_else(|| "<unlocked>".to_owned());
        table.push((row, resolved_version));
    }

    print_deps_table(&table, include_module_column);
    Ok(())
}

pub(crate) fn handle_outdated(module: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let selection = collect_dependency_rows(module)?;
    let lockfile = load_lockfile(&selection.lockfile_path)?;

    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let resolver = MavenResolver::new(paths)?;

    let packages = if let Some(selected_module) = module {
        select_packages_for_module(&resolver, &lockfile, &selection.rows, selected_module)?
    } else {
        lockfile.package.clone()
    };

    if packages.is_empty() {
        println!("no locked packages found");
        return Ok(());
    }

    let mut rows = Vec::with_capacity(packages.len());
    for package in packages {
        let coordinate = MavenCoordinate {
            group: package.group.clone(),
            artifact: package.artifact.clone(),
            version: Some(package.version.clone()),
            classifier: package.classifier.clone(),
        };
        let current = package.version;
        let name = format!("{}:{}", package.group, package.artifact);
        match resolver.latest_available_version(&coordinate) {
            Ok(Some(latest)) => {
                let status = if latest == current {
                    "up-to-date"
                } else {
                    "outdated"
                };
                rows.push((name, current, latest, status.to_owned()));
            }
            Ok(None) => {
                rows.push((name, current, "<unknown>".to_owned(), "unknown".to_owned()));
            }
            Err(_) => {
                rows.push((name, current, "<error>".to_owned(), "unknown".to_owned()));
            }
        }
    }

    print_outdated_table(&rows);
    Ok(())
}

fn collect_dependency_rows(
    module: Option<&str>,
) -> Result<DepsSelection, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        if let Some(selected) = module
            && !workspace
                .members
                .iter()
                .any(|member| member.module_name == selected)
        {
            return Err(format!("unknown workspace module `{selected}`").into());
        }

        let mut rows = Vec::new();
        for member in workspace.members {
            if module.is_some_and(|selected| selected != member.module_name) {
                continue;
            }

            let entries = read_declared_dependency_entries(&member.project.project_root)?;
            rows.extend(entries.into_iter().map(|entry| DirectDependencyRow {
                module: Some(member.module_name.clone()),
                name: entry.name,
                coordinate: entry.coordinate,
                scope: if entry.test { "test" } else { "main" },
            }));
        }

        return Ok(DepsSelection {
            rows,
            lockfile_path: workspace.root_dir.join(DEFAULT_LOCKFILE_NAME),
        });
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let project_file = nearest_project_file(&cwd)?;
    let entries = read_declared_dependency_entries(&cwd)?;
    Ok(DepsSelection {
        rows: entries
            .into_iter()
            .map(|entry| DirectDependencyRow {
                module: None,
                name: entry.name,
                coordinate: entry.coordinate,
                scope: if entry.test { "test" } else { "main" },
            })
            .collect(),
        lockfile_path: project_file
            .parent()
            .ok_or("project config path has no parent")?
            .join(DEFAULT_LOCKFILE_NAME),
    })
}

fn load_lockfile(path: &PathBuf) -> Result<Lockfile, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path).map_err(|_| {
        format!(
            "could not read lockfile at {}; run `jot lock` first",
            path.display()
        )
    })?;
    let lockfile = toml::from_str::<Lockfile>(&content)?;
    Ok(lockfile)
}

fn resolve_locked_version(lockfile: &Lockfile, coordinate: &str) -> Option<String> {
    let parsed = MavenCoordinate::parse(coordinate).ok()?;

    lockfile
        .roots
        .iter()
        .find(|root| same_package(root, &parsed))
        .and_then(|root| root.version.clone())
        .or_else(|| {
            lockfile
                .package
                .iter()
                .find(|package| {
                    package.group == parsed.group
                        && package.artifact == parsed.artifact
                        && package.classifier == parsed.classifier
                })
                .map(|package| package.version.clone())
        })
}

fn same_package(left: &MavenCoordinate, right: &MavenCoordinate) -> bool {
    left.group == right.group
        && left.artifact == right.artifact
        && left.classifier == right.classifier
}

fn print_deps_table(rows: &[(DirectDependencyRow, String)], include_module_column: bool) {
    let headers = if include_module_column {
        vec!["module", "name", "coordinate", "version", "scope"]
    } else {
        vec!["name", "coordinate", "version", "scope"]
    };

    let mut table_rows = Vec::with_capacity(rows.len());
    for (row, version) in rows {
        if include_module_column {
            table_rows.push(vec![
                row.module.as_deref().unwrap_or("-").to_owned(),
                row.name.clone(),
                row.coordinate.clone(),
                version.clone(),
                row.scope.to_owned(),
            ]);
        } else {
            table_rows.push(vec![
                row.name.clone(),
                row.coordinate.clone(),
                version.clone(),
                row.scope.to_owned(),
            ]);
        }
    }

    print_sharp_table(&headers, &table_rows);
}

fn print_outdated_table(rows: &[(String, String, String, String)]) {
    let headers = ["name", "current", "latest", "status"];
    let mut table_rows = Vec::with_capacity(rows.len());
    for (name, current, latest, status) in rows {
        table_rows.push(vec![
            name.to_owned(),
            current.to_owned(),
            latest.to_owned(),
            status.to_owned(),
        ]);
    }

    print_sharp_table(&headers, &table_rows);
}

fn select_packages_for_module(
    resolver: &MavenResolver,
    lockfile: &Lockfile,
    rows: &[DirectDependencyRow],
    selected_module: &str,
) -> Result<Vec<LockedPackage>, Box<dyn std::error::Error>> {
    let selected_rows = rows
        .iter()
        .filter(|row| row.module.as_deref() == Some(selected_module))
        .collect::<Vec<_>>();
    if selected_rows.is_empty() {
        return Ok(Vec::new());
    }

    let mut reachable = HashSet::new();
    for row in selected_rows {
        let root = resolver.resolve_coordinate(&row.coordinate)?;
        reachable.insert(root.to_string());

        for entry in resolver.resolve_dependency_tree(&row.coordinate, DEFAULT_LOCK_DEPTH)? {
            if entry.note.is_none() {
                reachable.insert(entry.coordinate.to_string());
            }
        }
    }

    let mut selected = lockfile
        .package
        .iter()
        .filter(|package| {
            let coord = MavenCoordinate {
                group: package.group.clone(),
                artifact: package.artifact.clone(),
                version: Some(package.version.clone()),
                classifier: package.classifier.clone(),
            };
            reachable.contains(&coord.to_string())
        })
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.artifact.cmp(&right.artifact))
            .then_with(|| left.version.cmp(&right.version))
    });
    selected.dedup_by(|left, right| {
        left.group == right.group
            && left.artifact == right.artifact
            && left.version == right.version
            && left.classifier == right.classifier
    });
    Ok(selected)
}

fn print_tree_entry(entry: &TreeEntry, base_depth: usize) {
    println!("{}", format_tree_entry(entry, base_depth));
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
