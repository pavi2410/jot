use jot_cache::JotPaths;
use jot_config::{pin_java_toolchain, read_toolchain_request};
use jot_toolchain::{InstallOptions, JavaToolchainRequest, ToolchainManager};

use crate::cli::{JavaCommand, JavaSubcommand};
use crate::commands::render::{StatusTone, print_status_stdout};
use crate::utils::{nearest_project_file, workspace_project_file};

pub(crate) fn handle_java(
    command: JavaCommand,
    manager: ToolchainManager,
    paths: JotPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    match command.command {
        JavaSubcommand::Install {
            version,
            vendor,
            force,
        } => {
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
        JavaSubcommand::List => {
            let active_request = read_toolchain_request(&std::env::current_dir()?)?;
            let installations = manager.list_installed()?;
            if installations.is_empty() {
                print_status_stdout(
                    "java",
                    StatusTone::Dim,
                    format!("no JDKs installed under {}", paths.jdks_dir().display()),
                );
                return Ok(());
            }

            for installation in installations {
                let marker = if active_request
                    .as_ref()
                    .is_some_and(|request| installation.matches_request(request))
                {
                    "*"
                } else {
                    " "
                };

                print_status_stdout(
                    "java",
                    if marker == "*" {
                        StatusTone::Accent
                    } else {
                        StatusTone::Info
                    },
                    format!(
                        "{} {:<9} {:<16} {:<18} {}",
                        marker,
                        installation.vendor,
                        installation.requested_version,
                        installation.release_name,
                        installation.java_home.display()
                    ),
                );
            }
        }
        JavaSubcommand::Pin {
            version,
            vendor,
            workspace,
        } => {
            let cwd = std::env::current_dir()?;
            let config_path = if workspace {
                workspace_project_file(&cwd)?
            } else {
                nearest_project_file(&cwd)?
            };
            pin_java_toolchain(&config_path, &JavaToolchainRequest { version, vendor })?;
            print_status_stdout(
                "pin",
                StatusTone::Success,
                format!("updated {}", config_path.display()),
            );
        }
    }

    Ok(())
}
