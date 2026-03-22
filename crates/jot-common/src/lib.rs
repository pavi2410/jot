mod archive;

use std::ffi::OsString;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

pub use archive::extract_archive;

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("unsupported archive format: {}", .0.display())]
    UnsupportedArchive(PathBuf),
}

// ── Filename sanitization ──────────────────────────────────────────────────

pub fn sanitize_for_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

// ── Progress indicators ────────────────────────────────────────────────────

pub fn spinner(message: &str) -> ProgressBar {
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .expect("valid spinner template")
            .tick_strings(&["-", "\\", "|", "/"]),
    );
    progress.enable_steady_tick(std::time::Duration::from_millis(100));
    progress.set_message(message.to_owned());
    progress
}

// ── SHA-256 file hashing ───────────────────────────────────────────────────

pub fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

// ── File collection ────────────────────────────────────────────────────────

/// Recursively collect files with the given extension from multiple directories.
/// Results are sorted and deduplicated.
pub fn collect_files_by_ext(dirs: &[PathBuf], ext: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in dirs {
        visit_files_by_ext(dir, ext, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

fn visit_files_by_ext(root: &Path, ext: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_files_by_ext(&path, ext, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            files.push(path);
        }
    }
}

// ── Classpath ──────────────────────────────────────────────────────────────

pub fn join_classpath(paths: &[PathBuf]) -> Result<OsString, std::env::JoinPathsError> {
    std::env::join_paths(paths)
}

// ── Environment ─────────────────────────────────────────────────────────────

/// Returns `true` when the `JOT_OFFLINE` environment variable is set to a truthy value.
pub fn offline_mode_enabled() -> bool {
    std::env::var("JOT_OFFLINE").ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
