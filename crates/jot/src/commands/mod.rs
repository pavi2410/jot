use clap::Parser;
use jot_cache::JotPaths;
use jot_toolchain::ToolchainManager;

use crate::cli::{Cli, Command};

pub(crate) mod audit;
pub(crate) mod build;
pub(crate) mod deps;
pub(crate) mod project;
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
        Command::Java(command) => toolchain::handle_java(command, manager, paths)?,
    }

    Ok(())
}
