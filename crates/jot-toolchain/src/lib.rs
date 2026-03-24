use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jot_cache::JotPaths;
use jot_platform::{OperatingSystem, Platform};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum JdkVendor {
    Adoptium,
    Corretto,
    Zulu,
    Oracle,
}

impl JdkVendor {
    pub fn as_adoptium_vendor(self) -> &'static str {
        match self {
            Self::Adoptium => "eclipse",
            Self::Corretto => "amazon",
            Self::Zulu => "azul",
            Self::Oracle => "oracle",
        }
    }
}

impl Display for JdkVendor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Adoptium => "adoptium",
            Self::Corretto => "corretto",
            Self::Zulu => "zulu",
            Self::Oracle => "oracle",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JavaToolchainRequest {
    pub version: String,
    pub vendor: Option<JdkVendor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct KotlinToolchainRequest {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledKotlin {
    pub version: String,
    pub kotlin_home: PathBuf,
    pub install_dir: PathBuf,
    pub installed_at: OffsetDateTime,
}

impl InstalledKotlin {
    pub fn kotlinc_binary(&self) -> PathBuf {
        let executable = if cfg!(windows) {
            "kotlinc.bat"
        } else {
            "kotlinc"
        };
        self.kotlin_home.join("bin").join(executable)
    }

    pub fn kotlin_stdlib_jar(&self) -> PathBuf {
        self.kotlin_home.join("lib").join("kotlin-stdlib.jar")
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InstallOptions {
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledJdk {
    pub vendor: JdkVendor,
    pub requested_version: String,
    pub release_name: String,
    pub semver: String,
    pub java_home: PathBuf,
    pub install_dir: PathBuf,
    pub platform: Platform,
    pub installed_at: OffsetDateTime,
}

impl InstalledJdk {
    pub fn matches_request(&self, request: &impl ToolchainRequest) -> bool {
        if let Some(vendor) = request.vendor()
            && vendor != self.vendor
        {
            return false;
        }

        let expected = request.version();
        self.requested_version == expected
            || self.release_name.starts_with(&format!("jdk-{expected}"))
            || self.semver.starts_with(expected)
    }

    pub fn java_binary(&self) -> PathBuf {
        binary_path(&self.java_home, "java")
    }

    pub fn javac_binary(&self) -> PathBuf {
        binary_path(&self.java_home, "javac")
    }

    pub fn jar_binary(&self) -> PathBuf {
        binary_path(&self.java_home, "jar")
    }

    pub fn javadoc_binary(&self) -> PathBuf {
        binary_path(&self.java_home, "javadoc")
    }
}

pub trait ToolchainRequest {
    fn version(&self) -> &str;
    fn vendor(&self) -> Option<JdkVendor>;
}

impl ToolchainRequest for JavaToolchainRequest {
    fn version(&self) -> &str {
        &self.version
    }

    fn vendor(&self) -> Option<JdkVendor> {
        self.vendor
    }
}

#[derive(Debug)]
pub struct ToolchainManager {
    client: Client,
    paths: JotPaths,
    platform: Platform,
    offline: bool,
}

impl ToolchainManager {
    const RESOLVE_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

    pub fn new(paths: JotPaths) -> Result<Self, ToolchainError> {
        let client = Client::builder().build()?;
        Ok(Self {
            client,
            paths,
            platform: Platform::current()?,
            offline: jot_common::offline_mode_enabled(),
        })
    }

    pub fn install(
        &self,
        request: &impl ToolchainRequest,
        options: InstallOptions,
    ) -> Result<InstalledJdk, ToolchainError> {
        let vendor = request.vendor().unwrap_or(JdkVendor::Adoptium);
        let lock_path = self.paths.install_lock_path(
            &vendor.to_string(),
            request.version(),
            &self.platform.to_string(),
        );
        let _install_lock = jot_common::FileLock::acquire(&lock_path).map_err(|source| {
            ToolchainError::LockAcquisition {
                path: lock_path,
                source,
            }
        })?;

        let resolve_progress =
            jot_common::spinner(&format!("Resolving JDK {} ({})", request.version(), vendor));
        let asset = self.resolve_latest_asset(request.version(), vendor)?;
        resolve_progress.finish_with_message(format!(
            "Resolved {} {} for {}",
            vendor, asset.release_name, self.platform
        ));
        let install_dir = self.paths.install_dir(
            &vendor.to_string(),
            &asset.release_name,
            &self.platform.to_string(),
        );
        let metadata_path = install_dir.join("install.json");

        if metadata_path.is_file() && !options.force {
            let existing = Self::read_installation(&metadata_path)?;
            if is_installation_usable(&existing) {
                return Ok(existing);
            }
            fs::remove_dir_all(&install_dir)?;
        }

        if install_dir.exists() && (options.force || !metadata_path.is_file()) {
            fs::remove_dir_all(&install_dir)?;
        }

        let download_path = self.paths.downloads_dir().join(&asset.binary.package.name);
        self.fetch_archive_checked(
            &asset.binary.package.link,
            &download_path,
            &asset.binary.package.checksum,
            options.force,
        )?;

        let temp_dir = TempDir::new_in(self.paths.jdks_dir())?;
        let extract_progress =
            jot_common::spinner(&format!("Extracting {}", asset.binary.package.name));
        jot_common::extract_archive(&download_path, temp_dir.path())?;
        extract_progress.finish_with_message(format!("Extracted {}", asset.binary.package.name));
        let java_home = detect_java_home(temp_dir.path())?;
        let installation = InstalledJdk {
            vendor,
            requested_version: request.version().to_owned(),
            release_name: asset.release_name,
            semver: asset.version.semver,
            java_home: install_dir.join(java_home.strip_prefix(temp_dir.path())?),
            install_dir: install_dir.clone(),
            platform: self.platform,
            installed_at: OffsetDateTime::now_utc(),
        };

        let extracted_dir = temp_dir.keep();
        fs::rename(extracted_dir, &install_dir)?;
        fs::write(&metadata_path, serde_json::to_vec_pretty(&installation)?)?;
        Ok(installation)
    }

    pub fn list_installed(&self) -> Result<Vec<InstalledJdk>, ToolchainError> {
        let mut installations = Vec::new();
        for entry in fs::read_dir(self.paths.jdks_dir())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let metadata_path = entry.path().join("install.json");
            if metadata_path.is_file() {
                installations.push(Self::read_installation(&metadata_path)?);
            }
        }

        installations.sort_by(|left, right| left.release_name.cmp(&right.release_name));
        Ok(installations)
    }

    pub fn list_installed_kotlin(&self) -> Result<Vec<InstalledKotlin>, ToolchainError> {
        let mut installations = Vec::new();
        let kotlins_dir = self.paths.kotlins_dir();
        if !kotlins_dir.is_dir() {
            return Ok(installations);
        }
        for entry in fs::read_dir(kotlins_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let metadata_path = entry.path().join("install.json");
            if metadata_path.is_file() {
                let content = fs::read(&metadata_path)?;
                let installed: InstalledKotlin = serde_json::from_slice(&content)?;
                installations.push(installed);
            }
        }
        installations.sort_by(|a, b| a.version.cmp(&b.version));
        Ok(installations)
    }

    pub fn ensure_installed(
        &self,
        request: &impl ToolchainRequest,
    ) -> Result<InstalledJdk, ToolchainError> {
        if let Some(existing) = self
            .list_installed()?
            .into_iter()
            .find(|installation| installation.matches_request(request))
        {
            return Ok(existing);
        }

        self.install(request, InstallOptions::default())
    }

    pub fn ensure_kotlin_installed(
        &self,
        request: &KotlinToolchainRequest,
    ) -> Result<InstalledKotlin, ToolchainError> {
        let install_dir = self.paths.kotlin_install_dir(&request.version);
        let metadata_path = install_dir.join("install.json");

        if metadata_path.is_file() {
            let content = fs::read(&metadata_path)?;
            let existing: InstalledKotlin = serde_json::from_slice(&content)?;
            if existing.kotlinc_binary().is_file() {
                return Ok(existing);
            }
            fs::remove_dir_all(&install_dir)?;
        }

        self.install_kotlin(request)
    }

    fn install_kotlin(
        &self,
        request: &KotlinToolchainRequest,
    ) -> Result<InstalledKotlin, ToolchainError> {
        let version = &request.version;
        let lock_path = self.paths.kotlin_install_lock_path(version);
        let _lock = jot_common::FileLock::acquire(&lock_path).map_err(|source| {
            ToolchainError::LockAcquisition {
                path: lock_path,
                source,
            }
        })?;

        let url = format!(
            "https://github.com/JetBrains/kotlin/releases/download/v{version}/kotlin-compiler-{version}.zip"
        );

        let download_filename = format!("kotlin-compiler-{version}.zip");
        let download_path = self.paths.downloads_dir().join(&download_filename);

        let resolve_progress = jot_common::spinner(&format!("Resolving Kotlin {version}"));
        resolve_progress.finish_with_message(format!("Resolved Kotlin {version}"));

        if self.offline && !download_path.is_file() {
            return Err(ToolchainError::OfflineDownloadRequired { url });
        }

        if !download_path.is_file() {
            self.download(&url, &download_path)?;
        }

        let install_dir = self.paths.kotlin_install_dir(version);
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir)?;
        }

        let extract_progress = jot_common::spinner(&format!("Extracting {download_filename}"));
        let temp_dir = TempDir::new_in(self.paths.kotlins_dir())?;
        jot_common::extract_archive(&download_path, temp_dir.path())?;
        extract_progress.finish_with_message(format!("Extracted {download_filename}"));

        let kotlin_home = install_dir.join("kotlinc");

        let extracted_dir = temp_dir.keep();
        fs::rename(extracted_dir, &install_dir)?;

        let installation = InstalledKotlin {
            version: version.clone(),
            kotlin_home: kotlin_home.clone(),
            install_dir: install_dir.clone(),
            installed_at: OffsetDateTime::now_utc(),
        };

        let metadata_path = install_dir.join("install.json");
        fs::write(&metadata_path, serde_json::to_vec_pretty(&installation)?)?;

        Ok(installation)
    }

    pub fn java_env(
        &self,
        installation: &InstalledJdk,
    ) -> Result<Vec<(String, String)>, ToolchainError> {
        let java_bin = installation.java_home.join("bin");
        let mut path_entries = vec![java_bin];
        if let Some(current_path) = std::env::var_os("PATH") {
            path_entries.extend(std::env::split_paths(&current_path));
        }

        let joined_path = std::env::join_paths(path_entries).map_err(ToolchainError::JoinPaths)?;
        Ok(vec![
            (
                "JAVA_HOME".into(),
                installation.java_home.to_string_lossy().into_owned(),
            ),
            ("PATH".into(), joined_path.to_string_lossy().into_owned()),
        ])
    }

    fn resolve_latest_asset(
        &self,
        feature_version: &str,
        vendor: JdkVendor,
    ) -> Result<AdoptiumAsset, ToolchainError> {
        let cache_path = self.resolve_cache_path(feature_version, vendor);
        if let Some(cached) = self.read_resolve_cache(&cache_path)? {
            return Ok(cached.asset);
        }

        if self.offline {
            if let Some(cached) = self.read_resolve_cache_any(&cache_path)? {
                return Ok(cached.asset);
            }
            return Err(ToolchainError::OfflineResolveCacheMiss {
                version: feature_version.to_owned(),
                platform: self.platform,
                vendor,
                cache_path,
            });
        }

        let url = format!(
            "https://api.adoptium.net/v3/assets/latest/{feature_version}/hotspot?release_type=ga&os={os}&architecture={arch}&image_type=jdk&vendor={vendor}",
            os = self.platform.os.as_adoptium(),
            arch = self.platform.arch.as_adoptium(),
            vendor = vendor.as_adoptium_vendor(),
        );
        let assets: Vec<AdoptiumAsset> = self.client.get(url).send()?.error_for_status()?.json()?;
        let asset = assets
            .into_iter()
            .next()
            .ok_or(ToolchainError::NoMatchingAsset {
                version: feature_version.to_owned(),
                platform: self.platform,
                vendor,
            })?;

        let cache_entry = ResolveCacheEntry {
            fetched_at: OffsetDateTime::now_utc(),
            asset: asset.clone(),
        };
        fs::write(cache_path, serde_json::to_vec_pretty(&cache_entry)?)?;

        Ok(asset)
    }

    fn resolve_cache_path(&self, feature_version: &str, vendor: JdkVendor) -> PathBuf {
        let safe_version = jot_common::sanitize_for_filename(feature_version);
        self.paths.resolve_cache_dir().join(format!(
            "asset-{vendor}-{version}-{platform}.json",
            vendor = vendor,
            version = safe_version,
            platform = self.platform,
        ))
    }

    fn read_resolve_cache(&self, path: &Path) -> Result<Option<ResolveCacheEntry>, ToolchainError> {
        let Some(entry) = self.read_resolve_cache_any(path)? else {
            return Ok(None);
        };
        if is_cache_fresh(
            entry.fetched_at,
            OffsetDateTime::now_utc(),
            Self::RESOLVE_CACHE_TTL,
        ) {
            return Ok(Some(entry));
        }

        Ok(None)
    }

    fn read_resolve_cache_any(
        &self,
        path: &Path,
    ) -> Result<Option<ResolveCacheEntry>, ToolchainError> {
        if !path.is_file() {
            return Ok(None);
        }

        let bytes = fs::read(path)?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    fn download(&self, url: &str, destination: &Path) -> Result<(), ToolchainError> {
        if self.offline {
            return Err(ToolchainError::OfflineDownloadRequired {
                url: url.to_owned(),
            });
        }

        let response = self.client.get(url).send()?.error_for_status()?;
        let total_bytes = response.content_length();
        let file_name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("archive");
        let progress = jot_common::download_bar(total_bytes, &format!("Downloading {file_name}"));
        jot_common::download_to_file(response, destination, Some(&progress))?;
        progress.finish_with_message(format!("Downloaded {file_name}"));
        Ok(())
    }

    fn fetch_archive_checked(
        &self,
        url: &str,
        destination: &Path,
        checksum: &str,
        force: bool,
    ) -> Result<(), ToolchainError> {
        if force && destination.exists() {
            fs::remove_file(destination)?;
        }

        if destination.is_file() {
            match self.verify_checksum(destination, checksum) {
                Ok(_) => return Ok(()),
                Err(ToolchainError::ChecksumMismatch { .. }) => {
                    if self.offline {
                        return Err(ToolchainError::OfflineArchiveMissing {
                            path: destination.to_path_buf(),
                            url: url.to_owned(),
                        });
                    }
                    fs::remove_file(destination)?;
                }
                Err(other) => return Err(other),
            }
        }

        if self.offline {
            return Err(ToolchainError::OfflineArchiveMissing {
                path: destination.to_path_buf(),
                url: url.to_owned(),
            });
        }

        self.download(url, destination)?;
        self.verify_checksum(destination, checksum)
    }

    fn verify_checksum(&self, path: &Path, expected: &str) -> Result<(), ToolchainError> {
        let actual = jot_common::sha256_file(path)?;
        if actual != expected {
            return Err(ToolchainError::ChecksumMismatch {
                path: path.to_path_buf(),
                expected: expected.to_owned(),
                actual,
            });
        }

        Ok(())
    }

    fn read_installation(path: &Path) -> Result<InstalledJdk, ToolchainError> {
        let content = fs::read(path)?;
        Ok(serde_json::from_slice(&content)?)
    }
}

fn detect_java_home(root: &Path) -> Result<PathBuf, ToolchainError> {
    let mut entries = fs::read_dir(root)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    let first = entries
        .into_iter()
        .find(|path| path.is_dir())
        .ok_or_else(|| ToolchainError::ExtractedArchiveMissingHome(root.to_path_buf()))?;

    if matches!(Platform::current()?.os, OperatingSystem::Mac) {
        let mac_home = first.join("Contents").join("Home");
        if mac_home.is_dir() {
            return Ok(mac_home);
        }
    }

    Ok(first)
}

fn is_installation_usable(installation: &InstalledJdk) -> bool {
    java_binary_path(&installation.java_home).is_file()
}

fn is_cache_fresh(fetched_at: OffsetDateTime, now: OffsetDateTime, ttl: Duration) -> bool {
    if now < fetched_at {
        return true;
    }

    let age = now - fetched_at;
    age.whole_seconds() <= ttl.as_secs() as i64
}

fn java_binary_path(java_home: &Path) -> PathBuf {
    binary_path(java_home, "java")
}

fn binary_path(java_home: &Path, tool: &str) -> PathBuf {
    let executable = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    };
    java_home.join("bin").join(executable)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
    release_name: String,
    version: AdoptiumVersion,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
    name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdoptiumVersion {
    semver: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ResolveCacheEntry {
    fetched_at: OffsetDateTime,
    asset: AdoptiumAsset,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolchainError {
    #[error("cache error: {0}")]
    Cache(#[from] jot_cache::CacheError),
    #[error("platform error: {0}")]
    Platform(#[from] jot_platform::PlatformError),
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to normalize PATH entries: {0}")]
    JoinPaths(#[source] std::env::JoinPathsError),
    #[error("failed to acquire install lock at {path}: {source}")]
    LockAcquisition {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("archive extraction failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error(
        "failed to resolve an Adoptium JDK for version {version}, vendor {vendor}, platform {platform}"
    )]
    NoMatchingAsset {
        version: String,
        platform: Platform,
        vendor: JdkVendor,
    },
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}", path = .path.display())]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("unsupported archive format: {0}")]
    UnsupportedArchive(PathBuf),
    #[error("could not detect JAVA_HOME in extracted archive: {0}")]
    ExtractedArchiveMissingHome(PathBuf),
    #[error("installed path {0} could not be made relative to the extraction directory")]
    StripPrefix(#[from] std::path::StripPrefixError),
    #[error(
        "offline mode is enabled and no cached JDK metadata was found for version {version} ({vendor}) on {platform}; run once online to populate {cache_path}",
        cache_path = .cache_path.display()
    )]
    OfflineResolveCacheMiss {
        version: String,
        platform: Platform,
        vendor: JdkVendor,
        cache_path: PathBuf,
    },
    #[error(
        "offline mode is enabled and archive cache is missing/invalid at {path}; run once online to fetch {url}",
        path = .path.display()
    )]
    OfflineArchiveMissing { path: PathBuf, url: String },
    #[error("offline mode is enabled and would need to download {url}")]
    OfflineDownloadRequired { url: String },
}

impl From<jot_common::CommonError> for ToolchainError {
    fn from(error: jot_common::CommonError) -> Self {
        match error {
            jot_common::CommonError::Io(error) => Self::Io(error),
            jot_common::CommonError::Zip(error) => Self::Zip(error),
            jot_common::CommonError::UnsupportedArchive(path) => Self::UnsupportedArchive(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InstalledJdk, JdkVendor, ToolchainRequest, is_cache_fresh, is_installation_usable,
        java_binary_path,
    };
    use jot_platform::{Architecture, OperatingSystem, Platform};
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::tempdir;
    use time::OffsetDateTime;

    struct Request {
        version: String,
        vendor: Option<JdkVendor>,
    }

    impl ToolchainRequest for Request {
        fn version(&self) -> &str {
            &self.version
        }

        fn vendor(&self) -> Option<JdkVendor> {
            self.vendor
        }
    }

    #[test]
    fn installation_matches_major_version_requests() {
        let installation = InstalledJdk {
            vendor: JdkVendor::Adoptium,
            requested_version: "21".into(),
            release_name: "jdk-21.0.10+7".into(),
            semver: "21.0.10+7.0.LTS".into(),
            java_home: PathBuf::from("/tmp/home"),
            install_dir: PathBuf::from("/tmp/install"),
            platform: Platform {
                os: OperatingSystem::Mac,
                arch: Architecture::Aarch64,
            },
            installed_at: OffsetDateTime::UNIX_EPOCH,
        };

        assert!(installation.matches_request(&Request {
            version: "21".into(),
            vendor: Some(JdkVendor::Adoptium),
        }));
    }

    #[test]
    fn vendor_mappings_match_expected_values() {
        assert_eq!(JdkVendor::Adoptium.as_adoptium_vendor(), "eclipse");
        assert_eq!(JdkVendor::Corretto.as_adoptium_vendor(), "amazon");
        assert_eq!(JdkVendor::Zulu.as_adoptium_vendor(), "azul");
        assert_eq!(JdkVendor::Oracle.as_adoptium_vendor(), "oracle");
    }

    #[test]
    fn installation_is_usable_only_when_java_binary_exists() {
        let temp = tempdir().expect("tempdir");
        let java_home = temp.path().join("jdk-home");
        fs::create_dir_all(java_home.join("bin")).expect("create bin directory");

        let installation = InstalledJdk {
            vendor: JdkVendor::Adoptium,
            requested_version: "21".into(),
            release_name: "jdk-21.0.10+7".into(),
            semver: "21.0.10+7.0.LTS".into(),
            java_home: java_home.clone(),
            install_dir: temp.path().join("install-dir"),
            platform: Platform {
                os: OperatingSystem::Mac,
                arch: Architecture::Aarch64,
            },
            installed_at: OffsetDateTime::UNIX_EPOCH,
        };

        assert!(!is_installation_usable(&installation));

        fs::write(java_binary_path(&java_home), "binary").expect("write fake java binary");
        assert!(is_installation_usable(&installation));
    }

    #[test]
    fn cache_freshness_respects_ttl_boundary() {
        let now = OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1000);
        let fetched_recent = now - time::Duration::seconds(60);
        let fetched_stale = now - time::Duration::hours(8);
        let ttl = Duration::from_secs(6 * 60 * 60);

        assert!(is_cache_fresh(fetched_recent, now, ttl));
        assert!(!is_cache_fresh(fetched_stale, now, ttl));
    }
}
