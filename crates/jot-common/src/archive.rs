use std::fs;
use std::path::Path;

use flate2::read::GzDecoder;
use zip::ZipArchive;

use crate::CommonError;

/// Extract a `.zip` or `.tar.gz` archive into the given destination directory.
pub fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), CommonError> {
    let file = fs::File::open(archive_path)?;
    let file_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if file_name.ends_with(".zip") {
        let mut archive = ZipArchive::new(file)?;
        archive.extract(destination)?;
        return Ok(());
    }

    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let decoder = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(destination)?;
        return Ok(());
    }

    Err(CommonError::UnsupportedArchive(archive_path.to_path_buf()))
}
