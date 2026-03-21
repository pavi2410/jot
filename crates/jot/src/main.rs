use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_devtools::DevTools;
use jot_config::{
    find_workspace_jot_toml, find_workspace_root_jot_toml, load_workspace_build_config,
    load_workspace_dependency_set, pin_java_toolchain, read_declared_dependencies,
    read_toolchain_request,
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
    Fmt {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        module: Option<String>,
    },
    Lint {
        #[arg(long)]
        module: Option<String>,
    },
    Audit {
        #[arg(long)]
        fix: bool,
        #[arg(long)]
        ci: bool,
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
        dependency: Option<String>,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long)]
        workspace: bool,
        #[arg(long)]
        module: Option<String>,
    },
    Run {
        #[arg(long)]
        module: Option<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    Test {
        #[arg(long)]
        module: Option<String>,
    },
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
        Command::Audit { fix, ci } => handle_audit(paths, fix, ci)?,
        Command::Build { module } => handle_build(paths, manager, module.as_deref())?,
        Command::Clean { global } => handle_clean(global, paths)?,
        Command::Fmt { check, module } => handle_fmt(paths, manager, check, module.as_deref())?,
        Command::Lint { module } => handle_lint(paths, manager, module.as_deref())?,
        Command::Lock {
            dependencies,
            depth,
            output,
        } => handle_lock(&dependencies, depth, &output)?,
        Command::Resolve { dependency, deps } => handle_resolve(&dependency, deps)?,
        Command::Run { module, args } => handle_run(paths, manager, module.as_deref(), &args)?,
        Command::Test { module } => handle_test(paths, manager, module.as_deref())?,
        Command::Tree {
            dependency,
            depth,
            workspace,
            module,
        } => handle_tree(dependency.as_deref(), depth, workspace, module.as_deref())?,
        Command::Java(command) => handle_java(command, manager, paths)?,
    }

    Ok(())
}

fn handle_lock(
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
    let resolver = MavenResolver::new(paths)?;
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
    std::fs::write(&output_path, content)?;
    println!("wrote {}", output_path.display());
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

fn handle_tree(
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

fn handle_fmt(
    paths: JotPaths,
    manager: ToolchainManager,
    check: bool,
    module: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let devtools = DevTools::new(resolver, manager)?;
    let cwd = std::env::current_dir()?;

    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        let members = if let Some(module) = module {
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

        let mut had_changes = false;
        for member in members {
            let report = devtools.format(&member, check)?;
            had_changes |= !report.changed_files.is_empty();
            println!(
                "{}: scanned {} Java files, {} {}",
                report.project.name,
                report.files_scanned,
                report.changed_files.len(),
                if check { "would change" } else { "changed" }
            );
            for path in report.changed_files {
                println!("  {}", path.display());
            }
        }

        if check && had_changes {
            return Err("format check failed".into());
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let report = devtools.format(&cwd, check)?;
    println!(
        "scanned {} Java files, {} {}",
        report.files_scanned,
        report.changed_files.len(),
        if check { "would change" } else { "changed" }
    );
    let has_changes = !report.changed_files.is_empty();
    for path in &report.changed_files {
        println!("{}", path.display());
    }
    if check && has_changes {
        return Err("format check failed".into());
    }
    Ok(())
}

fn handle_lint(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolver = MavenResolver::new(paths)?;
    let devtools = DevTools::new(resolver, manager)?;
    let cwd = std::env::current_dir()?;

    if let Some(workspace) = load_workspace_build_config(&cwd)? {
        let members = if let Some(module) = module {
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

        let mut violations = 0;
        for member in members {
            let report = devtools.lint(&member)?;
            println!(
                "{}: scanned {} Java files, {} violations",
                report.project.name,
                report.files_scanned,
                report.violations.len()
            );
            for violation in &report.violations {
                println!(
                    "{}:{}:{}: {} [{}] {}",
                    violation.path.display(),
                    violation.begin_line,
                    violation.begin_column,
                    violation.rule,
                    violation.ruleset,
                    violation.message
                );
            }
            for error in &report.processing_errors {
                eprintln!("{}: {}", error.path.display(), error.message);
            }
            violations += report.violations.len() + report.processing_errors.len();
        }
        if violations > 0 {
            return Err("lint found violations".into());
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let report = devtools.lint(&cwd)?;
    println!(
        "scanned {} Java files, {} violations",
        report.files_scanned,
        report.violations.len()
    );
    for violation in &report.violations {
        println!(
            "{}:{}:{}: {} [{}] {}",
            violation.path.display(),
            violation.begin_line,
            violation.begin_column,
            violation.rule,
            violation.ruleset,
            violation.message
        );
    }
    for error in &report.processing_errors {
        eprintln!("{}: {}", error.path.display(), error.message);
    }
    if !report.violations.is_empty() || !report.processing_errors.is_empty() {
        return Err("lint found violations".into());
    }
    Ok(())
}

fn handle_audit(
    paths: JotPaths,
    fix: bool,
    ci: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ToolchainManager::new(paths.clone())?;
    let resolver = MavenResolver::new(paths)?;
    let devtools = DevTools::new(resolver, manager)?;
    let cwd = std::env::current_dir()?;
    let report = devtools.audit(&cwd, fix)?;

    if report.findings.is_empty() {
        println!("No vulnerabilities found across {} packages", report.packages_scanned);
        return Ok(());
    }

    let mut ci_failure = false;
    for finding in &report.findings {
        ci_failure |= ci && finding.severity.is_ci_failure();
        println!(
            "{}  {}  {}",
            finding.severity.label(),
            finding.vuln_id,
            finding.package
        );
        println!("  {}", finding.summary);
        if let Some(version) = &finding.fixed_version {
            println!("  fixed in: {}", version);
        }
        if !finding.members.is_empty() {
            println!("  affected members: {}", finding.members.join(", "));
        }
        for chain in &finding.chains {
            let rendered = chain.iter().map(ToString::to_string).collect::<Vec<_>>().join(" -> ");
            println!("  chain: {}", rendered);
        }
    }

    if fix {
        println!("updated {} direct dependency declarations", report.fixed_dependencies);
    }

    if ci_failure {
        return Err("audit failed CI severity threshold".into());
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
        && !workspace.members.iter().any(|member| member.module_name == selected)
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