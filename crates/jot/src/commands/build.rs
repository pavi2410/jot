use std::path::Path;

use annotate_snippets::Level;
use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_config::{find_workspace_root_jot_toml, load_workspace_build_config};
use jot_devtools::{
    DevTools, FormatIssue, FormatReport, LintProcessingError, LintReport, LintViolation,
};
use jot_resolver::MavenResolver;
use jot_toolchain::ToolchainManager;

use crate::commands::render::{
    StatusTone, display_path, print_status_stderr, print_status_stdout, read_source_line,
    render_lint_processing_error as render_lint_processing_error_line, render_source_diagnostic,
    resolve_report_path, stderr_color, style,
};

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
            print_status_stdout(
                "build",
                StatusTone::Success,
                format!(
                    "{} {} -> {}",
                    module.build.project.name,
                    module.build.project.version,
                    display_path(&module.build.jar_path)
                ),
            );
            if let Some(path) = module.build.fat_jar_path {
                print_status_stdout(
                    "fat-jar",
                    StatusTone::Accent,
                    format!("{} -> {}", module.module_name, display_path(&path)),
                );
            }
            for warning in module.build.fat_jar_warnings {
                print_status_stderr("warn", StatusTone::Warning, warning);
            }
        }
        return Ok(());
    }

    if module.is_some() {
        return Err("--module can only be used from inside a workspace".into());
    }

    let output = builder.build(&cwd)?;
    print_status_stdout(
        "build",
        StatusTone::Success,
        format!(
            "{} {} -> {}",
            output.project.name,
            output.project.version,
            display_path(&output.jar_path)
        ),
    );
    if let Some(path) = output.fat_jar_path {
        print_status_stdout("fat-jar", StatusTone::Accent, display_path(&path));
    }
    for warning in output.fat_jar_warnings {
        print_status_stderr("warn", StatusTone::Warning, warning);
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
    let color = stderr_color();

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
    let color = stderr_color();

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
    let changed_verb = if report.checked {
        style("would change", StatusTone::Warning, color)
    } else {
        style("changed", StatusTone::Success, color)
    };
    print_status_stdout(
        "fmt",
        StatusTone::Info,
        format!(
            "{}: scanned {} files, {} {}",
            report.project.name,
            report.files_scanned,
            report.changed_files.len(),
            changed_verb
        ),
    );

    if report.checked {
        for issue in &report.issues {
            eprintln!("{}", render_format_issue(issue, color).trim_end());
        }
    } else {
        for path in &report.changed_files {
            print_status_stdout("changed", StatusTone::Accent, display_path(path));
        }
    }
}

fn print_lint_report(report: &LintReport, color: bool) {
    let tone = if report.violations.is_empty() && report.processing_errors.is_empty() {
        StatusTone::Success
    } else {
        StatusTone::Warning
    };
    print_status_stdout(
        "lint",
        tone,
        format!(
            "{}: scanned {} files, {} violations",
            report.project.name,
            report.files_scanned,
            report.violations.len()
        ),
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
    render_lint_processing_error_line(project_root, &error.path, &error.message)
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
