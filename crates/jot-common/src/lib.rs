mod archive;
mod lock;

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

pub use archive::extract_archive;
pub use lock::FileLock;

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

pub fn download_bar(total_bytes: Option<u64>, message: &str) -> ProgressBar {
    let progress = match total_bytes {
        Some(total) => ProgressBar::new(total),
        None => ProgressBar::new_spinner(),
    };

    let style = match total_bytes {
        Some(_) => ProgressStyle::with_template(
            "{spinner:.green} {msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
        )
        .expect("valid progress bar template")
        .progress_chars("=> "),
        None => ProgressStyle::with_template("{spinner:.green} {msg} {bytes} ({bytes_per_sec})")
            .expect("valid spinner template"),
    };

    progress.set_style(style);
    progress.set_message(message.to_owned());
    if total_bytes.is_none() {
        progress.enable_steady_tick(std::time::Duration::from_millis(100));
    }
    progress
}

pub fn count_bar(total: usize, message: &str) -> ProgressBar {
    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} [{bar:40.cyan/blue}] {pos}/{len}")
            .expect("valid progress bar template")
            .progress_chars("=> "),
    );
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
    let mut files: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|dir| {
            walkdir::WalkDir::new(dir)
                .into_iter()
                .filter_map(|e| e.ok())
        })
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some(ext))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files.dedup();
    files
}

// ── Streaming download to file ───────────────────────────────────────────────

/// Stream data from `reader` into `destination` via a temporary file, optionally
/// wrapping the reader with a progress bar. The temporary file is created in the
/// same directory as `destination` and atomically persisted.
pub fn download_to_file(
    reader: impl std::io::Read,
    destination: &Path,
    progress: Option<&ProgressBar>,
) -> Result<(), std::io::Error> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temp_file =
        tempfile::NamedTempFile::new_in(destination.parent().unwrap_or(Path::new(".")))?;

    match progress {
        Some(bar) => {
            let mut wrapped = bar.wrap_read(reader);
            std::io::copy(&mut wrapped, &mut temp_file)?;
        }
        None => {
            let mut reader = reader;
            std::io::copy(&mut reader, &mut temp_file)?;
        }
    }

    std::io::Write::flush(&mut temp_file)?;
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    temp_file
        .persist(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

// ── Atomic file writing ──────────────────────────────────────────────────────

/// Atomically write `content` to `path` by writing to a temporary file first,
/// then persisting it into place.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), std::io::Error> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no parent directory", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;
    let mut temp_file = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut temp_file, content)?;
    std::io::Write::flush(&mut temp_file)?;

    if path.exists() {
        fs::remove_file(path)?;
    }

    temp_file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

// ── Environment ─────────────────────────────────────────────────────────────

/// Returns `true` when the `JOT_OFFLINE` environment variable is set to a
/// truthy value (`1`, `true`, `yes`, or `on`).
pub fn offline_mode_enabled() -> bool {
    std::env::var("JOT_OFFLINE").ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
