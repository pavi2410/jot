use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_config::{
    find_workspace_jot_toml, find_workspace_root_jot_toml, load_workspace_build_config,
    pin_java_toolchain, read_declared_dependencies, read_toolchain_request,
};
use jot_resolver::{MavenResolver, TreeEntry};
use jot_toolchain::{InstallOptions, JavaToolchainRequest, JdkVendor, ToolchainManager};

#[derive(Debug, Parser)]
#[command(name = "jot", version, about = "A JVM toolchain manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build {
        #[arg(long)]
        module: Option<String>,
    },
    Clean {
        #[arg(long)]
        global: bool,
    },
    Lock {
        dependencies: Vec<String>,
        #[arg(long, default_value_t = 8)]
        depth: usize,
        #[arg(long, default_value = "jot.lock")]
        output: PathBuf,
    },
    Resolve {
        dependency: String,
        #[arg(long)]
        deps: bool,
    },
    Tree {
        dependency: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    Run {
        #[arg(long)]
        module: Option<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    Test,
    Java(JavaCommand),
}

#[derive(Debug, clap::Args)]
struct JavaCommand {
    #[command(subcommand)]
    command: JavaSubcommand,
}

#[derive(Debug, Subcommand)]
enum JavaSubcommand {
    Install {
        version: String,
        #[arg(long, default_value = "adoptium")]
        vendor: JdkVendor,
        #[arg(long)]
        force: bool,
    },
    List,
    Pin {
        version: String,
        #[arg(long)]
        vendor: Option<JdkVendor>,
        #[arg(long)]
        workspace: bool,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let manager = ToolchainManager::new(paths.clone())?;

    match cli.command {
        Command::Build { module } => handle_build(paths, manager, module.as_deref())?,
        Command::Clean { global } => handle_clean(global, paths)?,
        Command::Lock {
            dependencies,
            depth,
            output,
        } => handle_lock(&dependencies, depth, &output)?,
        Command::Resolve { dependency, deps } => handle_resolve(&dependency, deps)?,
        Command::Run { module, args } => handle_run(paths, manager, module.as_deref(), &args)?,
        Command::Test => handle_test(paths, manager)?,
        Command::Tree { dependency, depth } => handle_tree(&dependency, depth)?,
        Command::Java(command) => handle_java(command, manager, paths)?,
    }

    Ok(())
}

fn handle_lock(
    dependencies: &[String],
    depth: usize,
    output: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolved_inputs = if dependencies.is_empty() {
        let inputs = read_declared_dependencies(&std::env::current_dir()?)?;
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
    let resolver = MavenResolver::new(paths)?;
    let lockfile = resolver.resolve_lockfile(&resolved_inputs, depth)?;
    let content = toml::to_string_pretty(&lockfile)?;
    std::fs::write(output, content)?;
    println!("wrote {}", output.display());
    Ok(())
}

fn handle_resolve(dependency: &str, deps: bool) -> Result<(), Box<dyn std::error::Error>> {
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

fn handle_tree(dependency: &str, depth: usize) -> Result<(), Box<dyn std::error::Error>> {
    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let resolver = MavenResolver::new(paths)?;
    let entries = resolver.resolve_dependency_tree(dependency, depth)?;
    for entry in entries {
        print_tree_entry(&entry);
    }
    Ok(())
}

fn handle_build(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let cwd = std::env::current_dir()?;

    if find_workspace_root_jot_toml(&cwd)?.is_some() {
        let output = builder.build_workspace(&cwd, module)?;
        for module in output.modules {
            println!(
                "built {} {} at {}",
                module.build.project.name,
                module.build.project.version,
                module.build.jar_path.display()
            );
            if let Some(path) = module.build.fat_jar_path {
                println!("fat-jar ({}): {}", module.module_name, path.display());
            }
            for warning in module.build.fat_jar_warnings {
                eprintln!("warning: {warning}");
            }
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let output = builder.build(&cwd)?;
    println!(
        "built {} {} at {}",
        output.project.name,
        output.project.version,
        output.jar_path.display()
    );
    if let Some(path) = output.fat_jar_path {
        println!("fat-jar: {}", path.display());
    }
    for warning in output.fat_jar_warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

fn handle_run(
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

fn handle_test(
    paths: JotPaths,
    manager: ToolchainManager,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let cwd = std::env::current_dir()?;

    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        for member in workspace.members {
            let output = builder.test(&member.project.project_root)?;
            if output.tests_found {
                println!("test execution completed for {}", output.project.name);
            } else {
                println!("no tests found for {}", output.project.name);
            }
        }
        return Ok(());
    }

    let output = builder.test(&cwd)?;
    if output.tests_found {
        println!("test execution completed for {}", output.project.name);
    } else {
        println!("no tests found for {}", output.project.name);
    }
    Ok(())
}

fn print_tree_entry(entry: &TreeEntry) {
    let indent = "  ".repeat(entry.depth);
    let scope = entry.scope.clone().unwrap_or_else(|| "compile".to_owned());
    let optional = if entry.optional { " optional" } else { "" };
    let note = entry
        .note
        .as_ref()
        .map(|value| format!(" ({value})"))
        .unwrap_or_default();

    if entry.depth == 0 {
        println!("{}", entry.coordinate);
        return;
    }

    println!(
        "{}- {} [{}{}]{}",
        indent, entry.coordinate, scope, optional, note
    );
}

fn handle_clean(global: bool, paths: JotPaths) -> Result<(), Box<dyn std::error::Error>> {
    if !global {
        return Err("project-local clean is not implemented yet; use jot clean --global".into());
    }

    let summary = paths.clear_global_cache()?;
    println!(
        "Removed {} JDK entries and {} download entries from {}",
        summary.removed_jdk_entries,
        summary.removed_download_entries,
        paths.root().display()
    );
    Ok(())
}

fn handle_java(
    command: JavaCommand,
    manager: ToolchainManager,
    paths: JotPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    match command.command {
        JavaSubcommand::Install {
            version,
            vendor,
            force,
        } => {
            let installation = manager.install(
                &JavaToolchainRequest {
                    version,
                    vendor: Some(vendor),
                },
                InstallOptions { force },
            )?;
            println!(
                "installed {} {} at {}",
                installation.vendor,
                installation.release_name,
                installation.java_home.display()
            );
        }
        JavaSubcommand::List => {
            let active_request = read_toolchain_request(&std::env::current_dir()?)?;
            let installations = manager.list_installed()?;
            if installations.is_empty() {
                println!("No JDKs installed under {}", paths.jdks_dir().display());
                return Ok(());
            }

            for installation in installations {
                let marker = if active_request
                    .as_ref()
                    .is_some_and(|request| installation.matches_request(request))
                {
                    "*"
                } else {
                    " "
                };

                println!(
                    "{} {:<9} {:<16} {:<18} {}",
                    marker,
                    installation.vendor,
                    installation.requested_version,
                    installation.release_name,
                    installation.java_home.display()
                );
            }
        }
        JavaSubcommand::Pin {
            version,
            vendor,
            workspace,
        } => {
            let cwd = std::env::current_dir()?;
            let config_path = if workspace {
                workspace_project_file(&cwd)?
            } else {
                nearest_project_file(&cwd)?
            };
            pin_java_toolchain(
                &config_path,
                &JavaToolchainRequest { version, vendor },
            )?;
            println!("updated {}", config_path.display());
        }
    }

    Ok(())
}

fn nearest_project_file(start: &PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    jot_config::find_jot_toml(start)?.ok_or_else(|| {
        "could not find jot.toml in the current directory or any parent directory".into()
    })
}

fn workspace_project_file(start: &PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    find_workspace_jot_toml(start)?.ok_or_else(|| {
        "could not find a workspace jot.toml in the current directory or any parent directory".into()
    })
}