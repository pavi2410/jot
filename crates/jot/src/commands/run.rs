use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_config::{find_workspace_root_jot_toml, load_workspace_build_config};
use jot_resolver::MavenResolver;
use jot_toolchain::ToolchainManager;

pub(crate) fn handle_run(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let cwd = std::env::current_dir()?;

    if find_workspace_root_jot_toml(&cwd)?.is_some() {
        let module = module.ok_or("workspace run requires --module <name>")?;
        let output = builder.build_workspace(&cwd, Some(module))?;
        let selected = output
            .modules
            .into_iter()
            .find(|item| item.module_name == module)
            .ok_or("selected workspace module was not built")?;
        let fat_jar = selected
            .build
            .fat_jar_path
            .ok_or("selected module has no runnable main-class")?;

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
            return Err(format!("java exited with status {:?}", status.code()).into());
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    builder.run(&cwd, args)?;
    Ok(())
}

pub(crate) fn handle_test(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let cwd = std::env::current_dir()?;

    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        let selected = if let Some(module) = module {
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

        for project_root in selected {
            let output = builder.test(&project_root)?;
            if output.tests_found {
                println!("test execution completed for {}", output.project.name);
            } else {
                println!("no tests found for {}", output.project.name);
            }
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let output = builder.test(&cwd)?;
    if output.tests_found {
        println!("test execution completed for {}", output.project.name);
    } else {
        println!("no tests found for {}", output.project.name);
    }
    Ok(())
}
