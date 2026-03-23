use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_config::find_workspace_root_jot_toml;
use jot_resolver::MavenResolver;
use jot_toolchain::ToolchainManager;

use crate::commands::render::{StatusTone, print_status_stdout};

pub(crate) fn handle_run(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
    args: &[String],
) -> Result<(), anyhow::Error> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let cwd = std::env::current_dir()?;

    if find_workspace_root_jot_toml(&cwd)?.is_some() {
        let module =
            module.ok_or_else(|| anyhow::anyhow!("workspace run requires --module <name>"))?;
        let output = builder.build_workspace(&cwd, Some(module))?;
        let selected = output
            .modules
            .into_iter()
            .find(|item| item.module_name == module)
            .ok_or_else(|| anyhow::anyhow!("selected workspace module was not built"))?;
        let fat_jar = selected
            .build
            .fat_jar_path
            .ok_or_else(|| anyhow::anyhow!("selected module has no runnable main-class"))?;

        let status = std::process::Command::new(selected.build.installed_jdk.java_binary())
            .current_dir(selected.build.project.project_root)
            .arg("-jar")
            .arg(fat_jar)
            .args(args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if !status.success() {
            anyhow::bail!("java exited with status {:?}", status.code());
        }
        return Ok(());
    }

    if module.is_some() {
        anyhow::bail!("--module can only be used from inside a workspace");
    }

    builder.run(&cwd, args)?;
    Ok(())
}

pub(crate) fn handle_test(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
) -> Result<(), anyhow::Error> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let targets = super::select_project_targets(module)?;
    let roots = match targets {
        super::ProjectTargets::Workspace { roots } => roots,
        super::ProjectTargets::Single { root } => vec![root],
    };

    for project_root in roots {
        let output = builder.test(&project_root)?;
        if output.tests_found {
            print_status_stdout(
                "test",
                StatusTone::Success,
                format!("execution completed for {}", output.project.name),
            );
        } else {
            print_status_stdout(
                "test",
                StatusTone::Dim,
                format!("no tests found for {}", output.project.name),
            );
        }
    }
    Ok(())
}
