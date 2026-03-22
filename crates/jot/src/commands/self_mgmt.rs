use std::fs;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::IsTerminal;
use std::io::Write;
use std::path::{Path, PathBuf};

use jot_cache::JotPaths;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use tempfile::{NamedTempFile, TempDir};

use crate::cli::{SelfCommand, SelfSubcommand};
use crate::commands::render::{StatusTone, print_status_stdout, stdout_color, style};
use crate::utils::find_file_named;

const DEFAULT_RELEASE_REPO: &str = "pavi2410/jot";
const CHECKSUM_ASSET_NAME: &str = "SHA256SUMS";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug)]
struct ReleaseAssetSelection<'a> {
    archive: &'a GithubReleaseAsset,
    checksums: &'a GithubReleaseAsset,
}

pub(crate) fn handle_self(
    command: SelfCommand,
    paths: JotPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    match command.command {
        SelfSubcommand::Update {
            version,
            check,
            yes,
        } => handle_self_update(paths, version.as_deref(), check, yes),
        SelfSubcommand::Uninstall { yes } => handle_self_uninstall(yes),
    }
}

fn handle_self_update(
    paths: JotPaths,
    requested_version: Option<&str>,
    check_only: bool,
    yes: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("JOT_OFFLINE").is_ok() {
        return Err("cannot run `jot self update` in offline mode".into());
    }

    let release_repo = std::env::var("JOT_RELEASE_REPO")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASE_REPO.to_owned());
    let (target_triple, archive_extension) = current_release_target()?;

    let client = Client::builder().build()?;
    let release = fetch_release(&client, &release_repo, requested_version)?;
    let release_version = normalize_version(&release.tag_name);
    let archive_name = format!(
        "jot-{}-{target_triple}.{archive_extension}",
        release.tag_name
    );
    let current_version = env!("CARGO_PKG_VERSION");

    if check_only {
        print_status_stdout(
            "self",
            StatusTone::Info,
            format!("current: {current_version}"),
        );
        print_status_stdout(
            "self",
            StatusTone::Info,
            format!("latest: {release_version}"),
        );
        print_status_stdout("self", StatusTone::Info, format!("repo:   {release_repo}"));
        return Ok(());
    }

    if requested_version.is_none() && semver_not_newer(&release_version, current_version) {
        print_status_stdout(
            "self",
            StatusTone::Success,
            format!("already up to date ({current_version})"),
        );
        return Ok(());
    }

    let selection = select_release_assets(&release, &archive_name)?;

    if !yes && io::stdin().is_terminal() {
        println!(
            "{}",
            style(
                &format!("Update jot from {current_version} to {release_version}? [y/N]"),
                StatusTone::Warning,
                stdout_color(),
            )
        );
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        let decision = answer.trim().to_ascii_lowercase();
        if decision != "y" && decision != "yes" {
            print_status_stdout("self", StatusTone::Dim, "aborted");
            return Ok(());
        }
    }

    let archive_path = paths.downloads_dir().join(&archive_name);
    let checksums_path = paths
        .downloads_dir()
        .join(format!("jot-{release_version}-{CHECKSUM_ASSET_NAME}"));

    download_to_path(
        &client,
        &selection.archive.browser_download_url,
        &archive_path,
    )?;
    download_to_path(
        &client,
        &selection.checksums.browser_download_url,
        &checksums_path,
    )?;

    verify_download_checksum(&archive_path, &checksums_path, &archive_name)?;
    let extracted_binary = extract_release_binary(&archive_path)?;
    self_replace::self_replace(extracted_binary)?;

    print_status_stdout(
        "self",
        StatusTone::Success,
        format!("updated from {current_version} to {release_version}"),
    );
    Ok(())
}

fn handle_self_uninstall(yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !yes {
        if !io::stdin().is_terminal() {
            return Err("non-interactive uninstall requires --yes".into());
        }

        println!(
            "{}",
            style(
                "Uninstall jot from this executable path? [y/N]",
                StatusTone::Warning,
                stdout_color(),
            )
        );
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        let decision = answer.trim().to_ascii_lowercase();
        if decision != "y" && decision != "yes" {
            print_status_stdout("self", StatusTone::Dim, "aborted");
            return Ok(());
        }
    }

    let executable = std::env::current_exe()?;
    self_replace::self_delete()?;
    print_status_stdout(
        "self",
        StatusTone::Success,
        format!(
            "scheduled uninstall of {} (binary removed after process exit)",
            executable.display()
        ),
    );
    Ok(())
}

fn fetch_release(
    client: &Client,
    release_repo: &str,
    version: Option<&str>,
) -> Result<GithubRelease, Box<dyn std::error::Error>> {
    let endpoint = match version {
        Some(value) => {
            let normalized = normalize_tag(value);
            format!("https://api.github.com/repos/{release_repo}/releases/tags/{normalized}")
        }
        None => format!("https://api.github.com/repos/{release_repo}/releases/latest"),
    };

    let release = client
        .get(endpoint)
        .header("User-Agent", "jot-upgrade")
        .send()?
        .error_for_status()?
        .json::<GithubRelease>()?;
    Ok(release)
}

fn select_release_assets<'a>(
    release: &'a GithubRelease,
    archive_name: &str,
) -> Result<ReleaseAssetSelection<'a>, Box<dyn std::error::Error>> {
    let archive = release
        .assets
        .iter()
        .find(|asset| asset.name == archive_name)
        .ok_or_else(|| {
            format!(
                "release {} does not contain required asset {}",
                release.tag_name, archive_name
            )
        })?;
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == CHECKSUM_ASSET_NAME)
        .ok_or_else(|| {
            format!(
                "release {} does not contain required asset {}",
                release.tag_name, CHECKSUM_ASSET_NAME
            )
        })?;

    Ok(ReleaseAssetSelection { archive, checksums })
}

fn download_to_path(
    client: &Client,
    url: &str,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut response = client
        .get(url)
        .header("User-Agent", "jot-upgrade")
        .send()?
        .error_for_status()?;
    let mut temp = NamedTempFile::new_in(destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no parent", destination.display()),
        )
    })?)?;

    io::copy(&mut response, &mut temp)?;
    temp.flush()?;
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    temp.persist(destination).map_err(|error| error.error)?;
    Ok(())
}

fn verify_download_checksum(
    archive_path: &Path,
    checksums_path: &Path,
    archive_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = read_expected_checksum(checksums_path, archive_name)?;
    let actual = sha256_file(archive_path)?;

    if expected != actual {
        return Err(format!(
            "checksum mismatch for {}: expected {}, got {}",
            archive_name, expected, actual
        )
        .into());
    }
    Ok(())
}

fn read_expected_checksum(
    checksums_path: &Path,
    archive_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let file = fs::File::open(checksums_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(file_name) = parts.next() else {
            continue;
        };

        let file_name = file_name.trim_start_matches('*');
        if file_name == archive_name {
            return Ok(hash.to_owned());
        }
    }

    Err(format!(
        "did not find checksum for {} in {}",
        archive_name,
        checksums_path.display()
    )
    .into())
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(jot_common::sha256_file(path)?)
}

fn extract_release_binary(archive_path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    jot_common::extract_archive(archive_path, temp_dir.path())?;

    let binary_name = if cfg!(windows) { "jot.exe" } else { "jot" };
    let extracted_path = find_file_named(temp_dir.path(), binary_name)?.ok_or_else(|| {
        format!(
            "could not locate {} in {}",
            binary_name,
            archive_path.display()
        )
    })?;
    let staged_binary = temp_dir.path().join(format!("{}-staged", binary_name));
    fs::copy(&extracted_path, &staged_binary)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&staged_binary)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&staged_binary, permissions)?;
    }

    let kept_path = temp_dir.keep();
    Ok(kept_path.join(format!("{}-staged", binary_name)))
}

fn current_release_target() -> Result<(&'static str, &'static str), Box<dyn std::error::Error>> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("linux", "x86_64") => Ok(("x86_64-unknown-linux-musl", "tar.gz")),
        ("macos", "x86_64") => Ok(("x86_64-apple-darwin", "tar.gz")),
        ("macos", "aarch64") => Ok(("aarch64-apple-darwin", "tar.gz")),
        ("windows", "x86_64") => Ok(("x86_64-pc-windows-msvc", "zip")),
        _ => Err(format!("unsupported upgrade platform: {os}-{arch}").into()),
    }
}

fn normalize_tag(version: &str) -> String {
    if version.starts_with('v') {
        version.to_owned()
    } else {
        format!("v{version}")
    }
}

fn normalize_version(tag: &str) -> String {
    tag.trim_start_matches('v').to_owned()
}

fn semver_not_newer(candidate: &str, baseline: &str) -> bool {
    let Ok(candidate) = Version::parse(candidate) else {
        return false;
    };
    let Ok(baseline) = Version::parse(baseline) else {
        return false;
    };

    candidate <= baseline
}
