use jot_cache::JotPaths;
use jot_config::{pin_java_toolchain, read_toolchain_request};
use jot_toolchain::{
    InstallOptions, JavaToolchainRequest, JdkVendor, KotlinToolchainRequest, ToolchainManager,
};

use crate::cli::{ToolchainCommand, ToolchainSubcommand};
use crate::commands::render::{
    StatusTone, display_path, print_sharp_table, print_status_stdout, stdout_color, style,
};
use crate::utils::{nearest_project_file, workspace_project_file};

/// Parsed `tool@version` specifier.
enum ToolchainSpec {
    Java { version: String, vendor: JdkVendor },
    Kotlin { version: String },
}

/// Parse a specifier like `java@21`, `java@corretto-21`, or `kotlin@2.1.0`.
///
/// For Java, the version part can optionally be prefixed with a vendor name:
///   - `21` or `openjdk-21`  → Adoptium (default)
///   - `corretto-21`         → Corretto
///   - `zulu-21`             → Zulu
///   - `oracle-21`           → Oracle
///   - `adoptium-21`         → Adoptium (explicit)
///   - `temurin-21`          → Adoptium (alias)
fn parse_toolchain_spec(spec: &str) -> Result<ToolchainSpec, anyhow::Error> {
    let (name, version_part) = spec
        .split_once('@')
        .ok_or_else(|| anyhow::anyhow!("expected <tool>@<version> (e.g. java@21), got `{spec}`"))?;

    if version_part.is_empty() {
        anyhow::bail!("missing version in `{spec}`");
    }

    match name {
        "java" => {
            let (vendor, version) = parse_java_version(version_part);
            Ok(ToolchainSpec::Java { version, vendor })
        }
        "kotlin" => Ok(ToolchainSpec::Kotlin {
            version: version_part.to_string(),
        }),
        other => Err(anyhow::anyhow!(
            "unknown toolchain `{other}` (expected java or kotlin)"
        )),
    }
}

/// Parse an optional vendor prefix from a Java version string.
///
/// Returns `(vendor, bare_version)`.
fn parse_java_version(version_part: &str) -> (JdkVendor, String) {
    // Try to split on the first hyphen: "corretto-21" → ("corretto", "21")
    if let Some((prefix, rest)) = version_part.split_once('-') {
        match prefix {
            "adoptium" | "temurin" | "openjdk" => return (JdkVendor::Adoptium, rest.to_string()),
            "corretto" => return (JdkVendor::Corretto, rest.to_string()),
            "zulu" => return (JdkVendor::Zulu, rest.to_string()),
            "oracle" => return (JdkVendor::Oracle, rest.to_string()),
            _ => {} // not a vendor prefix, treat whole string as version
        }
    }

    // No vendor prefix — default to Adoptium
    (JdkVendor::Adoptium, version_part.to_string())
}

pub(crate) fn handle_toolchain(
    command: ToolchainCommand,
    manager: ToolchainManager,
    _paths: JotPaths,
) -> Result<(), anyhow::Error> {
    match command.command {
        ToolchainSubcommand::Install { toolchain, force } => {
            let spec = parse_toolchain_spec(&toolchain)?;
            match spec {
                ToolchainSpec::Java { version, vendor } => {
                    let installation = manager.install(
                        &JavaToolchainRequest {
                            version,
                            vendor: Some(vendor),
                        },
                        InstallOptions { force },
                    )?;
                    print_status_stdout(
                        "install",
                        StatusTone::Success,
                        format!(
                            "{} {} -> {}",
                            installation.vendor,
                            installation.release_name,
                            installation.java_home.display()
                        ),
                    );
                }
                ToolchainSpec::Kotlin { version } => {
                    let installed = manager.ensure_kotlin_installed(&KotlinToolchainRequest {
                        version: version.clone(),
                    })?;
                    print_status_stdout(
                        "install",
                        StatusTone::Success,
                        format!("kotlin {} -> {}", version, installed.kotlin_home.display()),
                    );
                }
            }
        }
        ToolchainSubcommand::List => {
            let jdks = manager.list_installed()?;
            let kotlins = manager.list_installed_kotlin()?;

            if jdks.is_empty() && kotlins.is_empty() {
                print_status_stdout("toolchain", StatusTone::Dim, "no toolchains installed");
                return Ok(());
            }

            let active_request = read_toolchain_request(&std::env::current_dir()?)?;
            let color = stdout_color();
            let mut rows = Vec::new();

            for jdk in jdks {
                let is_active = active_request
                    .as_ref()
                    .is_some_and(|r| jdk.matches_request(r));
                let marker = if is_active {
                    style("*", StatusTone::Success, color)
                } else {
                    " ".to_string()
                };
                rows.push(vec![
                    marker,
                    format!("java@{}-{}", jdk.vendor, jdk.requested_version),
                    jdk.release_name,
                ]);
            }

            for kotlin in kotlins {
                rows.push(vec![
                    " ".to_string(),
                    format!("kotlin@{}", kotlin.version),
                    kotlin.version.clone(),
                ]);
            }

            print_status_stdout("toolchain", StatusTone::Info, "installed toolchains");
            print_sharp_table(&["", "toolchain", "version"], &rows);
        }
        ToolchainSubcommand::Pin {
            toolchain,
            workspace,
        } => {
            let spec = parse_toolchain_spec(&toolchain)?;
            let cwd = std::env::current_dir()?;
            let config_path = if workspace {
                workspace_project_file(&cwd)?
            } else {
                nearest_project_file(&cwd)?
            };
            match spec {
                ToolchainSpec::Java { version, vendor } => {
                    pin_java_toolchain(
                        &config_path,
                        &JavaToolchainRequest {
                            version,
                            vendor: Some(vendor),
                        },
                    )?;
                }
                ToolchainSpec::Kotlin { .. } => {
                    // TODO: implement pin_kotlin_toolchain in jot-config
                    anyhow::bail!("kotlin pin is not yet implemented");
                }
            }
            print_status_stdout(
                "pin",
                StatusTone::Success,
                format!("updated {}", display_path(&config_path)),
            );
        }
    }

    Ok(())
}
