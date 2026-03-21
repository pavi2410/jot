use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::IsTerminal;
use std::io::Write;
use std::path::{Path, PathBuf};

mod init_templates;

use annotate_snippets::renderer::DecorStyle;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use clap::{Parser, Subcommand};
use fs2::FileExt;
use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_config::{
    find_workspace_jot_toml, find_workspace_root_jot_toml, load_workspace_build_config,
    load_workspace_dependency_set, pin_java_toolchain, read_declared_dependencies,
    read_toolchain_request,
};
use jot_devtools::{
    AuditFinding, AuditReport, AuditSeverity, DevTools, FormatIssue, FormatReport,
    LintProcessingError, LintReport, LintViolation,
};
use jot_resolver::{MavenResolver, TreeEntry};
use jot_toolchain::{InstallOptions, JavaToolchainRequest, JdkVendor, ToolchainManager};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
#[command(name = "jot", version, about = "A JVM toolchain manager")]
struct Cli {
    #[arg(long, global = true)]
    offline: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build {
        #[arg(long)]
        module: Option<String>,
    },
    Init {
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long = "package")]
        package_name: Option<String>,
        name: Option<String>,
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
        Command::Audit { fix, ci } => handle_audit(paths, fix, ci)?,
        Command::Build { module } => handle_build(paths, manager, module.as_deref())?,
        Command::Init {
            template,
            group,
            package_name,
            name,
        } => handle_init(
            &std::env::current_dir()?,
            template,
            group,
            package_name,
            name,
        )?,
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
    let color = std::io::stderr().is_terminal();

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
            print_format_report(&report, color);
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
    print_format_report(&report, color);
    let has_changes = !report.changed_files.is_empty();
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
    let color = std::io::stderr().is_terminal();

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
            print_lint_report(&report, color);
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
    print_lint_report(&report, color);
    if !report.violations.is_empty() || !report.processing_errors.is_empty() {
        return Err("lint found violations".into());
    }
    Ok(())
}

fn handle_audit(paths: JotPaths, fix: bool, ci: bool) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ToolchainManager::new(paths.clone())?;
    let resolver = MavenResolver::new(paths)?;
    let devtools = DevTools::new(resolver, manager)?;
    let cwd = std::env::current_dir()?;
    let report = devtools.audit(&cwd, fix)?;

    if report.findings.is_empty() {
        println!(
            "No vulnerabilities found across {} packages",
            report.packages_scanned
        );
        return Ok(());
    }

    let ci_failure = report
        .findings
        .iter()
        .any(|finding| ci && finding.severity.is_ci_failure());
    print_audit_report(&report, ci, std::io::stdout().is_terminal());

    if fix {
        println!(
            "updated {} direct dependency declarations",
            report.fixed_dependencies
        );
    }

    if ci_failure {
        return Err("audit failed CI severity threshold".into());
    }

    Ok(())
}

fn print_format_report(report: &FormatReport, color: bool) {
    println!(
        "{}: scanned {} Java files, {} {}",
        report.project.name,
        report.files_scanned,
        report.changed_files.len(),
        if report.checked {
            "would change"
        } else {
            "changed"
        }
    );

    if report.checked {
        for issue in &report.issues {
            eprintln!("{}", render_format_issue(issue, color).trim_end());
        }
    } else {
        for path in &report.changed_files {
            println!("  {}", path.display());
        }
    }
}

fn print_lint_report(report: &LintReport, color: bool) {
    println!(
        "{}: scanned {} Java files, {} violations",
        report.project.name,
        report.files_scanned,
        report.violations.len()
    );

    for violation in &report.violations {
        eprintln!(
            "{}",
            render_lint_violation(&report.project.project_root, violation, color).trim_end()
        );
    }
    for error in &report.processing_errors {
        eprintln!(
            "{}",
            render_lint_processing_error(&report.project.project_root, error).trim_end()
        );
    }
}

fn print_audit_report(report: &AuditReport, ci: bool, color: bool) {
    let mut by_severity = BTreeMap::new();
    for severity in [
        AuditSeverity::Critical,
        AuditSeverity::High,
        AuditSeverity::Moderate,
        AuditSeverity::Low,
        AuditSeverity::Unknown,
    ] {
        by_severity.insert(severity, 0_usize);
    }
    for finding in &report.findings {
        *by_severity.entry(finding.severity).or_default() += 1;
    }

    println!("Audit summary");
    println!("  packages scanned: {}", report.packages_scanned);
    println!("  findings: {}", report.findings.len());
    println!(
        "  severities: critical={} high={} moderate={} low={} unknown={}",
        by_severity[&AuditSeverity::Critical],
        by_severity[&AuditSeverity::High],
        by_severity[&AuditSeverity::Moderate],
        by_severity[&AuditSeverity::Low],
        by_severity[&AuditSeverity::Unknown],
    );
    println!();

    for finding in &report.findings {
        print_audit_finding(finding, ci, color);
    }
}

fn print_audit_finding(finding: &AuditFinding, ci: bool, color: bool) {
    println!(
        "{} {}",
        severity_badge(finding.severity, color),
        finding.vuln_id
    );
    println!("  package: {}", finding.package);
    println!("  summary: {}", finding.summary);
    if let Some(version) = &finding.fixed_version {
        println!("  fixed version: {}", version);
    }
    if !finding.members.is_empty() {
        println!("  affected members: {}", finding.members.join(", "));
    }
    if ci && finding.severity.is_ci_failure() {
        println!("  ci gate: this finding fails --ci");
    }
    for (index, chain) in finding.chains.iter().enumerate() {
        let rendered = chain
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" -> ");
        println!("  path {}: {}", index + 1, rendered);
    }
    println!();
}

fn severity_badge(severity: AuditSeverity, color: bool) -> String {
    let label = format!("[{}]", severity.label());
    if !color {
        return label;
    }

    let code = match severity {
        AuditSeverity::Critical => "1;31",
        AuditSeverity::High => "31",
        AuditSeverity::Moderate => "33",
        AuditSeverity::Low => "34",
        AuditSeverity::Unknown => "90",
    };
    format!("\u{1b}[{code}m{label}\u{1b}[0m")
}

fn render_format_issue(issue: &FormatIssue, color: bool) -> String {
    let expected = compact_preview(&issue.expected_line, 100);
    render_source_diagnostic(
        &issue.path,
        issue.line,
        issue.column,
        issue.column + 1,
        &issue.actual_line,
        Level::ERROR,
        "file is not formatted",
        &format!("formatter output diverges here; expected `{expected}`"),
        color,
    )
}

fn render_lint_violation(project_root: &Path, violation: &LintViolation, color: bool) -> String {
    let path = resolve_report_path(project_root, &violation.path);
    let source_line = read_source_line(&path, violation.begin_line).unwrap_or_default();
    let level = if violation.priority <= 2 {
        Level::ERROR
    } else {
        Level::WARNING
    };
    let label = format!(
        "{} [{}], priority {}",
        violation.rule, violation.ruleset, violation.priority
    );
    render_source_diagnostic(
        &path,
        violation.begin_line,
        violation.begin_column,
        violation.end_column.max(violation.begin_column + 1),
        &source_line,
        level,
        &violation.message,
        &label,
        color,
    )
}

fn render_lint_processing_error(project_root: &Path, error: &LintProcessingError) -> String {
    let path = resolve_report_path(project_root, &error.path);
    format!("error: {}: {}", path.display(), error.message)
}

fn render_source_diagnostic(
    path: &Path,
    line: usize,
    begin_column: usize,
    end_column: usize,
    source_line: &str,
    level: Level,
    title: &str,
    label: &str,
    color: bool,
) -> String {
    let renderer = if color {
        Renderer::styled().decor_style(DecorStyle::Unicode)
    } else {
        Renderer::plain()
    };
    let (start, end) = snippet_span(source_line, begin_column, end_column);
    let rendered_path = path.display().to_string();
    let snippet = Snippet::source(source_line)
        .line_start(line)
        .path(&rendered_path)
        .annotation(AnnotationKind::Primary.span(start..end).label(label));
    renderer
        .render(&[level.primary_title(title).element(snippet)])
        .to_string()
}

fn resolve_report_path(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn read_source_line(path: &Path, line_number: usize) -> Option<String> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .nth(line_number.saturating_sub(1))
        .map(|line| line.to_owned())
}

fn snippet_span(source_line: &str, begin_column: usize, end_column: usize) -> (usize, usize) {
    if source_line.is_empty() {
        return (0, 0);
    }

    let start = begin_column.saturating_sub(1).min(source_line.len() - 1);
    let requested_end = end_column.saturating_sub(1).max(begin_column);
    let end = requested_end.min(source_line.len()).max(start + 1);
    (start, end)
}

fn compact_preview(value: &str, max_len: usize) -> String {
    let compact = value.trim();
    if compact.chars().count() <= max_len {
        return compact.to_owned();
    }

    compact
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>()
        + "..."
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

fn handle_init(
    cwd: &Path,
    template: Option<String>,
    group: Option<String>,
    package_name: Option<String>,
    name: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = init_templates::InitOptions {
        template,
        group,
        package_name,
        name,
    };
    let output = init_templates::scaffold(cwd, options)?;
    println!(
        "created {} template at {} ({} files)",
        output.template,
        output.root.display(),
        output.created_files
    );
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
            pin_java_toolchain(&config_path, &JavaToolchainRequest { version, vendor })?;
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
        "could not find a workspace jot.toml in the current directory or any parent directory"
            .into()
    })
}

fn write_locked_file(
    paths: &JotPaths,
    output_path: &Path,
    content: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path = paths.locks_dir().join(format!(
        "file-{}.lock",
        sanitize_for_filename(&output_path.to_string_lossy())
    ));
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    lock_file.lock_exclusive()?;

    let parent = output_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no parent directory", output_path.display()),
        )
    })?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content)?;
    temp_file.flush()?;

    if output_path.exists() {
        fs::remove_file(output_path)?;
    }
    temp_file
        .persist(output_path)
        .map_err(|error| error.error)?;

    let _ = lock_file.unlock();
    Ok(())
}

fn sanitize_for_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}
