use annotate_snippets::renderer::DecorStyle;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};

use crate::errors::{DiagnosticSeverity, JavacDiagnostic};

pub(crate) fn format_javac_stderr(raw: &str, color: bool) -> String {
    let diagnostics = parse_javac_diagnostics(raw);
    if diagnostics.is_empty() {
        return raw.to_owned();
    }

    let renderer = if color {
        Renderer::styled().decor_style(DecorStyle::Unicode)
    } else {
        Renderer::plain()
    };

    let mut output = String::from("javac diagnostics\n");
    for diagnostic in diagnostics {
        output.push_str(&render_diagnostic(&renderer, &diagnostic));
        output.push('\n');
    }

    output.trim_end().to_owned()
}

pub(crate) fn render_diagnostic(renderer: &Renderer, diagnostic: &JavacDiagnostic) -> String {
    let level = match diagnostic.severity {
        DiagnosticSeverity::Error => Level::ERROR,
        DiagnosticSeverity::Warning => Level::WARNING,
    };

    if let Some(source_line) = diagnostic.source_line.as_ref() {
        let (span_start, span_end) = diagnostic
            .caret_line
            .as_ref()
            .and_then(|line| caret_span(line))
            .unwrap_or((0, 0));
        let snippet = Snippet::source(source_line)
            .line_start(diagnostic.line)
            .path(&diagnostic.path)
            .annotation(
                AnnotationKind::Primary
                    .span(span_start..span_end)
                    .label(&diagnostic.message),
            );
        renderer
            .render(&[level.primary_title(&diagnostic.message).element(snippet)])
            .to_string()
    } else {
        let snippet = Snippet::source("")
            .line_start(diagnostic.line)
            .path(&diagnostic.path)
            .annotation(
                AnnotationKind::Primary
                    .span(0..0)
                    .label(&diagnostic.message),
            );
        renderer
            .render(&[level.primary_title(&diagnostic.message).element(snippet)])
            .to_string()
    }
}

pub(crate) fn caret_span(line: &str) -> Option<(usize, usize)> {
    let start = line.find('^')?;
    let end_exclusive = line.rfind('^').map(|end| end + 1).unwrap_or(start + 1);
    Some((start, end_exclusive))
}

pub(crate) fn parse_javac_diagnostics(raw: &str) -> Vec<JavacDiagnostic> {
    let mut diagnostics = Vec::new();
    let lines = raw.lines().collect::<Vec<_>>();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].trim_end();
        if let Some((path, line_number, severity, message)) = parse_diagnostic_header(line) {
            let mut source_line = None;
            let mut caret_line = None;

            if index + 1 < lines.len() {
                let candidate = lines[index + 1].trim_end();
                if !candidate.contains(": error:") && !candidate.contains(": warning:") {
                    source_line = Some(candidate.to_owned());
                    index += 1;

                    if index + 1 < lines.len() {
                        let caret_candidate = lines[index + 1].trim_end();
                        if caret_candidate.contains('^') {
                            caret_line = Some(caret_candidate.to_owned());
                            index += 1;
                        }
                    }
                }
            }

            diagnostics.push(JavacDiagnostic {
                path,
                line: line_number,
                severity,
                message,
                source_line,
                caret_line,
            });
        }

        index += 1;
    }

    diagnostics
}

fn parse_diagnostic_header(line: &str) -> Option<(String, usize, DiagnosticSeverity, String)> {
    let (severity, marker) = if line.contains(": error: ") {
        (DiagnosticSeverity::Error, ": error: ")
    } else if line.contains(": warning: ") {
        (DiagnosticSeverity::Warning, ": warning: ")
    } else {
        return None;
    };

    let marker_idx = line.find(marker)?;
    let location = &line[..marker_idx];
    let message = line[marker_idx + marker.len()..].trim().to_owned();
    let split_idx = location.rfind(':')?;
    let path = location[..split_idx].to_owned();
    let line_number = location[split_idx + 1..].parse::<usize>().ok()?;

    Some((path, line_number, severity, message))
}
