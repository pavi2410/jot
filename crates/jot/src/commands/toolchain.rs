use jot_cache::JotPaths;
use jot_config::{pin_java_toolchain, read_toolchain_request};
use jot_toolchain::{InstallOptions, JavaToolchainRequest, ToolchainManager};

use crate::cli::{JavaCommand, JavaSubcommand};
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
            println!(
                "installed {} {} at {}",
                installation.vendor,
                installation.release_name,
                installation.java_home.display()
            );
        }
        JavaSubcommand::List => {
            let active_request = read_toolchain_request(&std::env::current_dir()?)?;
            let installations = manager.list_installed()?;
            if installations.is_empty() {
                println!("No JDKs installed under {}", paths.jdks_dir().display());
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

                println!(
                    "{} {:<9} {:<16} {:<18} {}",
                    marker,
                    installation.vendor,
                    installation.requested_version,
                    installation.release_name,
                    installation.java_home.display()
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
            println!("updated {}", config_path.display());
        }
    }

    Ok(())
}
