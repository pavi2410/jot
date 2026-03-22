use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use jot_config::{WorkspaceBuildConfig, load_project_build_config};

use crate::errors::BuildError;
use crate::graph::{GraphError, WorkspaceGraph};
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
    let graph = workspace_graph(workspace)?;
    match graph.topological_order() {
        Ok(order) => Ok(order),
        Err(GraphError::CycleDetected) => Err(BuildError::WorkspaceCycleDetected),
        Err(GraphError::UnknownModule(module)) => Err(BuildError::UnknownWorkspaceModule(module)),
    }
}

fn select_modules_for_build(
    workspace: &WorkspaceBuildConfig,
    module: Option<&str>,
) -> Result<BTreeSet<String>, BuildError> {
    let graph = workspace_graph(workspace)?;

    match module {
        Some(requested) => graph
            .dependency_closure(requested)
            .map_err(map_graph_error_to_build),
        None => Ok(graph.modules()),
    }
}

fn workspace_graph(workspace: &WorkspaceBuildConfig) -> Result<WorkspaceGraph, BuildError> {
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

    let mut graph = WorkspaceGraph::with_modules(
        workspace
            .members
            .iter()
            .map(|member| member.module_name.clone()),
    );

    for member in &workspace.members {
        for dependency_path in &member.project.path_dependencies {
            let dependency = root_to_module
                .get(&canonical_root(dependency_path))
                .ok_or_else(|| BuildError::UnknownWorkspaceDependency {
                    module: member.module_name.clone(),
                    path: dependency_path.clone(),
                })?;
            graph
                .add_dependency(&member.module_name, dependency)
                .map_err(map_graph_error_to_build)?;
        }
    }

    Ok(graph)
}

fn map_graph_error_to_build(error: GraphError) -> BuildError {
    match error {
        GraphError::UnknownModule(module) => BuildError::UnknownWorkspaceModule(module),
        GraphError::CycleDetected => BuildError::WorkspaceCycleDetected,
    }
}
