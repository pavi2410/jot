use std::path::{Path, PathBuf};

use annotate_snippets::renderer::DecorStyle;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use jot_resolver::TreeEntry;
use tabled::{builder::Builder, settings::Style};

pub(crate) fn print_sharp_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut builder = Builder::default();
    builder.push_record(headers.iter().copied());
    for row in rows {
        builder.push_record(row.iter().map(String::as_str));
    }

    println!("{}", builder.build().with(Style::sharp()));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_source_diagnostic(
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

pub(crate) fn render_lint_processing_error(
    project_root: &Path,
    path: &Path,
    message: &str,
) -> String {
    let resolved_path = resolve_report_path(project_root, path);
    format!("error: {}: {}", resolved_path.display(), message)
}

pub(crate) fn resolve_report_path(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

pub(crate) fn read_source_line(path: &Path, line_number: usize) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .nth(line_number.saturating_sub(1))
        .map(|line| line.to_owned())
}

pub(crate) fn format_tree_entry(entry: &TreeEntry, base_depth: usize) -> String {
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
            return entry.coordinate.to_string();
        }
        return format!("{}- {}", indent, entry.coordinate);
    }

    format!(
        "{}- {} [{}{}]{}",
        indent, entry.coordinate, scope, optional, note
    )
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
