use std::io::IsTerminal;
use std::path::Path;

use annotate_snippets::renderer::DecorStyle;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_config::{find_workspace_root_jot_toml, load_workspace_build_config};
use jot_devtools::{
    DevTools, FormatIssue, FormatReport, LintProcessingError, LintReport, LintViolation,
};
use jot_resolver::MavenResolver;
use jot_toolchain::ToolchainManager;

pub(crate) fn handle_build(
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

pub(crate) fn handle_fmt(
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

pub(crate) fn handle_lint(
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

fn resolve_report_path(project_root: &Path, path: &Path) -> std::path::PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn read_source_line(path: &Path, line_number: usize) -> Option<String> {
    std::fs::read_to_string(path)
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
