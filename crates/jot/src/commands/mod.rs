use std::path::PathBuf;

use clap::Parser;
use jot_cache::JotPaths;
use jot_config::load_workspace_build_config;
use jot_toolchain::ToolchainManager;

use crate::cli::{Cli, Command};

pub(crate) mod audit;
pub(crate) mod build;
pub(crate) mod deps;
pub(crate) mod project;
pub(crate) mod publish;
pub(crate) mod render;
pub(crate) mod run;
pub(crate) mod self_mgmt;
pub(crate) mod toolchain;

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if cli.offline {
        // Safe here because jot is single-process CLI setup before any worker threads spawn.
        unsafe {
            std::env::set_var("JOT_OFFLINE", "1");
        }
    }

    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let manager = ToolchainManager::new(paths.clone())?;

    match cli.command {
        Command::Audit { fix, ci } => audit::handle_audit(paths, fix, ci)?,
        Command::Build { module } => build::handle_build(paths, manager, module.as_deref())?,
        Command::Publish {
            module,
            repository,
            username,
            password,
            signing_key,
            dry_run,
            allow_snapshot,
        } => publish::handle_publish(
            paths,
            manager,
            module.as_deref(),
            repository.as_deref(),
            username.as_deref(),
            password.as_deref(),
            signing_key.as_deref(),
            dry_run,
            allow_snapshot,
        )?,
        Command::Init {
            template,
            group,
            package_name,
            name,
        } => project::handle_init(
            &std::env::current_dir()?,
            template,
            group,
            package_name,
            name,
        )?,
        Command::Clean { global } => project::handle_clean(global, paths)?,
        Command::Add {
            coordinate,
            catalog,
            test,
            name,
        } => deps::handle_add(
            coordinate.as_deref(),
            catalog.as_deref(),
            test,
            name.as_deref(),
        )?,
        Command::Remove { name, test } => deps::handle_remove(&name, test)?,
        Command::Deps { module } => deps::handle_deps(module.as_deref())?,
        Command::Outdated { module } => deps::handle_outdated(module.as_deref())?,
        Command::Fmt { check, module } => {
            build::handle_fmt(paths, manager, check, module.as_deref())?
        }
        Command::Lint { module } => build::handle_lint(paths, manager, module.as_deref())?,
        Command::Lock {
            dependencies,
            depth,
            output,
        } => deps::handle_lock(&dependencies, depth, &output)?,
        Command::Resolve { dependency, deps } => deps::handle_resolve(&dependency, deps)?,
        Command::Run { module, args } => run::handle_run(paths, manager, module.as_deref(), &args)?,
        Command::Test { module } => run::handle_test(paths, manager, module.as_deref())?,
        Command::SelfCmd(command) => self_mgmt::handle_self(command, paths)?,
        Command::Tree {
            dependency,
            depth,
            workspace,
            module,
        } => deps::handle_tree(dependency.as_deref(), depth, workspace, module.as_deref())?,
        Command::Toolchain(command) => toolchain::handle_toolchain(command, manager, paths)?,
    }

    Ok(())
}

/// Resolved project targets: either workspace members or a single project.
pub(crate) enum ProjectTargets {
    Workspace { roots: Vec<PathBuf> },
    Single { root: PathBuf },
}

/// Resolve which project roots to operate on, handling workspace detection and `--module` filtering.
pub(crate) fn select_project_targets(
    module: Option<&str>,
) -> Result<ProjectTargets, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        let roots = if let Some(module) = module {
            let member = workspace
                .members
                .iter()
                .find(|candidate| candidate.module_name == module)
                .ok_or_else(|| format!("unknown workspace module `{module}`"))?;
            vec![member.project.project_root.clone()]
        } else {
            workspace
                .members
                .iter()
                .map(|member| member.project.project_root.clone())
                .collect::<Vec<_>>()
        };
        return Ok(ProjectTargets::Workspace { roots });
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    Ok(ProjectTargets::Single { root: cwd })
}
