use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error(
        "invalid Maven coordinate {0}; expected group:artifact, group:artifact:version, or group:artifact:version:classifier"
    )]
    InvalidCoordinate(String),
    #[error("cache error: {0}")]
    Cache(#[from] jot_cache::CacheError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Xml(#[from] quick_xml::DeError),
    #[error("version metadata is missing for {0}")]
    MissingVersionMetadata(String),
    #[error("cannot compute POM URL because version is missing for {0}")]
    MissingVersionForPom(String),
    #[error("cannot compute artifact URL because version is missing for {0}")]
    MissingVersionForArtifact(String),
    #[error("unsupported or unresolvable version expression: {0}")]
    UnsupportedVersionExpression(String),
    #[error("invalid parent POM declaration: {0}")]
    InvalidParentPom(String),
    #[error("detected a cycle while resolving POM model: {0}")]
    PomCycleDetected(String),
    #[error("checksum mismatch for {coordinate}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        coordinate: String,
        expected: String,
        actual: String,
    },
    #[error(
        "offline mode is enabled and required cache entry is missing for {url}; run once online to populate {cache_path}",
        cache_path = .cache_path.display()
    )]
    OfflineCacheMiss { url: String, cache_path: PathBuf },
    #[error(
        "offline mode is enabled and artifact {coordinate} is not cached at {path}; run once online to download it",
        path = .path.display()
    )]
    OfflineArtifactMissing { coordinate: String, path: PathBuf },
    #[error("offline mode is enabled and would need to download artifact {0}")]
    OfflineDownloadRequired(String),
    #[error("failed to acquire cache lock at {path}: {source}")]
    LockAcquisition {
        path: PathBuf,
        source: std::io::Error,
    },
}
