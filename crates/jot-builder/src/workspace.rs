use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use jot_config::{WorkspaceBuildConfig, load_project_build_config};

use crate::errors::BuildError;
use crate::{BuildOutput, JavaProjectBuilder};

#[derive(Debug)]
pub struct WorkspaceBuildOutput {
    pub modules: Vec<WorkspaceModuleBuildOutput>,
}

#[derive(Debug)]
pub struct WorkspaceModuleBuildOutput {
    pub module_name: String,
    pub build: BuildOutput,
}

impl JavaProjectBuilder {
    pub fn build_workspace(
        &self,
        start: &std::path::Path,
        module: Option<&str>,
    ) -> Result<WorkspaceBuildOutput, BuildError> {
        let workspace = jot_config::load_workspace_build_config(start)?
            .ok_or_else(|| BuildError::WorkspaceNotFound(start.to_path_buf()))?;
        let order = workspace_build_order(&workspace)?;

        let selected_modules = select_modules_for_build(&workspace, module)?;

        let mut cache = HashMap::<PathBuf, BuildOutput>::new();
        let mut stack = Vec::<PathBuf>::new();
        let mut modules = Vec::new();

        for module_name in order {
            if !selected_modules.contains(&module_name) {
                continue;
            }
            let member = workspace
                .members
                .iter()
                .find(|candidate| candidate.module_name == module_name)
                .ok_or_else(|| BuildError::UnknownWorkspaceModule(module_name.clone()))?;
            let build =
                self.build_project_with_cache(member.project.clone(), &mut cache, &mut stack)?;
            modules.push(WorkspaceModuleBuildOutput {
                module_name: module_name.clone(),
                build,
            });
        }

        Ok(WorkspaceBuildOutput { modules })
    }

    pub(crate) fn build_project_with_cache(
        &self,
        project: jot_config::ProjectBuildConfig,
        cache: &mut HashMap<PathBuf, BuildOutput>,
        stack: &mut Vec<PathBuf>,
    ) -> Result<BuildOutput, BuildError> {
        let project_root = project.project_root.canonicalize()?;
        if let Some(cached) = cache.get(&project_root) {
            return Ok(cached.clone());
        }

        if stack.contains(&project_root) {
            let cycle = stack
                .iter()
                .chain(std::iter::once(&project_root))
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(BuildError::PathDependencyCycle(cycle));
        }

        stack.push(project_root.clone());
        let mut path_dependency_jars = Vec::new();
        for dependency_root in &project.path_dependencies {
            let dependency_project = load_project_build_config(dependency_root)?;
            let dependency_output =
                self.build_project_with_cache(dependency_project, cache, stack)?;
            path_dependency_jars.push(dependency_output.jar_path.clone());
        }

        let output = self.build_project_internal(project, &path_dependency_jars)?;
        cache.insert(project_root, output.clone());
        stack.pop();
        Ok(output)
    }
}

fn canonical_root(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn workspace_build_order(workspace: &WorkspaceBuildConfig) -> Result<Vec<String>, BuildError> {
    let root_to_module = workspace
        .members
        .iter()
        .map(|member| {
            (
                canonical_root(&member.project.project_root),
                member.module_name.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut incoming = workspace
        .members
        .iter()
        .map(|member| (member.module_name.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = workspace
        .members
        .iter()
        .map(|member| (member.module_name.clone(), Vec::<String>::new()))
        .collect::<BTreeMap<_, _>>();

    for member in &workspace.members {
        for dependency_path in &member.project.path_dependencies {
            let dependency = root_to_module
                .get(&canonical_root(dependency_path))
                .ok_or_else(|| BuildError::UnknownWorkspaceDependency {
                    module: member.module_name.clone(),
                    path: dependency_path.clone(),
                })?
                .clone();
            adjacency
                .entry(dependency)
                .or_default()
                .push(member.module_name.clone());
            *incoming.entry(member.module_name.clone()).or_default() += 1;
        }
    }

    let mut ready = incoming
        .iter()
        .filter_map(|(module, degree)| (*degree == 0).then_some(module.clone()))
        .collect::<Vec<_>>();
    ready.sort();

    let mut order = Vec::new();
    while let Some(module) = ready.pop() {
        order.push(module.clone());
        let mut neighbors = adjacency.remove(&module).unwrap_or_default();
        neighbors.sort();
        neighbors.reverse();
        for dependent in neighbors {
            if let Some(value) = incoming.get_mut(&dependent) {
                *value = value.saturating_sub(1);
                if *value == 0 {
                    ready.push(dependent);
                }
            }
        }
    }

    if order.len() != workspace.members.len() {
        return Err(BuildError::WorkspaceCycleDetected);
    }

    Ok(order)
}

fn select_modules_for_build(
    workspace: &WorkspaceBuildConfig,
    module: Option<&str>,
) -> Result<BTreeSet<String>, BuildError> {
    let mut roots = workspace
        .members
        .iter()
        .map(|member| {
            (
                member.module_name.clone(),
                canonical_root(&member.project.project_root),
            )
        })
        .collect::<BTreeMap<_, _>>();

    if module.is_none() {
        return Ok(roots.keys().cloned().collect());
    }

    let requested = module.expect("module checked as some").to_owned();
    let root = roots
        .remove(&requested)
        .ok_or_else(|| BuildError::UnknownWorkspaceModule(requested.clone()))?;

    let by_root = workspace
        .members
        .iter()
        .map(|member| {
            (
                canonical_root(&member.project.project_root),
                member.module_name.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let by_module = workspace
        .members
        .iter()
        .map(|member| (member.module_name.clone(), member))
        .collect::<BTreeMap<_, _>>();

    let mut selected = BTreeSet::new();
    let mut stack = vec![
        by_root
            .get(&root)
            .expect("root is in workspace map")
            .clone(),
    ];

    while let Some(next) = stack.pop() {
        if !selected.insert(next.clone()) {
            continue;
        }
        let member = by_module
            .get(&next)
            .ok_or_else(|| BuildError::UnknownWorkspaceModule(next.clone()))?;
        for dep_root in &member.project.path_dependencies {
            let dep = by_root
                .get(&canonical_root(dep_root))
                .ok_or_else(|| BuildError::UnknownWorkspaceDependency {
                    module: next.clone(),
                    path: dep_root.clone(),
                })?
                .clone();
            stack.push(dep);
        }
    }

    Ok(selected)
}
