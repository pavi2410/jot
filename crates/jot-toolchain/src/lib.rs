use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use jot_cache::JotPaths;
use jot_platform::{OperatingSystem, Platform};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use time::OffsetDateTime;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum JdkVendor {
    Adoptium,
}

impl JdkVendor {
    pub fn as_adoptium_vendor(self) -> &'static str {
        match self {
            Self::Adoptium => "eclipse",
        }
    }
}

impl Display for JdkVendor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Adoptium => "adoptium",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JavaToolchainRequest {
    pub version: String,
    pub vendor: Option<JdkVendor>,
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
        if let Some(vendor) = request.vendor() {
            if vendor != self.vendor {
                return false;
            }
        }

        let expected = request.version();
        self.requested_version == expected
            || self.release_name.starts_with(&format!("jdk-{expected}"))
            || self.semver.starts_with(expected)
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
}

impl ToolchainManager {
    pub fn new(paths: JotPaths) -> Result<Self, ToolchainError> {
        let client = Client::builder().build()?;
        Ok(Self {
            client,
            paths,
            platform: Platform::current()?,
        })
    }

    pub fn install(
        &self,
        request: &impl ToolchainRequest,
        options: InstallOptions,
    ) -> Result<InstalledJdk, ToolchainError> {
        let resolve_progress = spinner(&format!(
            "Resolving JDK {} ({})",
            request.version(),
            request.vendor().unwrap_or(JdkVendor::Adoptium)
        ));
        let vendor = request.vendor().unwrap_or(JdkVendor::Adoptium);
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
            return Self::read_installation(&metadata_path);
        }

        if install_dir.exists() && options.force {
            fs::remove_dir_all(&install_dir)?;
        }

        let download_path = self.paths.downloads_dir().join(&asset.binary.package.name);
        if !download_path.is_file() || options.force {
            self.download(&asset.binary.package.link, &download_path)?;
        }
        self.verify_checksum(&download_path, &asset.binary.package.checksum)?;

        let temp_dir = TempDir::new_in(self.paths.jdks_dir())?;
        let extract_progress = spinner(&format!("Extracting {}", asset.binary.package.name));
        self.extract_archive(&download_path, temp_dir.path())?;
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
        let url = format!(
            "https://api.adoptium.net/v3/assets/latest/{feature_version}/hotspot?release_type=ga&os={os}&architecture={arch}&image_type=jdk&vendor={vendor}",
            os = self.platform.os.as_adoptium(),
            arch = self.platform.arch.as_adoptium(),
            vendor = vendor.as_adoptium_vendor(),
        );
        let assets: Vec<AdoptiumAsset> = self.client.get(url).send()?.error_for_status()?.json()?;
        assets.into_iter().next().ok_or(ToolchainError::NoMatchingAsset {
            version: feature_version.to_owned(),
            platform: self.platform,
            vendor,
        })
    }

    fn download(&self, url: &str, destination: &Path) -> Result<(), ToolchainError> {
        let mut response = self.client.get(url).send()?.error_for_status()?;
        let total_bytes = response.content_length();
        let progress = download_bar(
            total_bytes,
            &format!(
                "Downloading {}",
                destination
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("archive")
            ),
        );
        let mut file = fs::File::create(destination)?;
        let mut buffer = [0_u8; 64 * 1024];

        loop {
            let bytes_read = response.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])?;
            progress.inc(bytes_read as u64);
        }

        progress.finish_with_message(format!(
            "Downloaded {}",
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("archive")
        ));
        Ok(())
    }

    fn verify_checksum(&self, path: &Path, expected: &str) -> Result<(), ToolchainError> {
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

        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            return Err(ToolchainError::ChecksumMismatch {
                path: path.to_path_buf(),
                expected: expected.to_owned(),
                actual,
            });
        }

        Ok(())
    }

    fn extract_archive(&self, archive_path: &Path, destination: &Path) -> Result<(), ToolchainError> {
        let file = fs::File::open(archive_path)?;
        let file_name = archive_path.file_name().and_then(|name| name.to_str()).unwrap_or_default();

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

        Err(ToolchainError::UnsupportedArchive(archive_path.to_path_buf()))
    }

    fn read_installation(path: &Path) -> Result<InstalledJdk, ToolchainError> {
        let content = fs::read(path)?;
        Ok(serde_json::from_slice(&content)?)
    }
}

fn spinner(message: &str) -> ProgressBar {
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

fn download_bar(total_bytes: Option<u64>, message: &str) -> ProgressBar {
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

#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
    release_name: String,
    version: AdoptiumVersion,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct AdoptiumVersion {
    semver: String,
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
    #[error("archive extraction failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("failed to resolve an Adoptium JDK for version {version}, vendor {vendor}, platform {platform}")]
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
}

#[cfg(test)]
mod tests {
    use super::{InstalledJdk, JdkVendor, ToolchainRequest};
    use jot_platform::{Architecture, OperatingSystem, Platform};
    use std::path::PathBuf;
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
}