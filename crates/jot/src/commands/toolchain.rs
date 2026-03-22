use jot_cache::JotPaths;
use jot_config::{pin_java_toolchain, read_toolchain_request};
use jot_toolchain::{InstallOptions, JavaToolchainRequest, ToolchainManager};

use crate::cli::{JavaCommand, JavaSubcommand};
use crate::commands::render::{
    StatusTone, display_path, print_sharp_table, print_status_stdout, stdout_color, style,
};
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

            let color = stdout_color();
            let mut rows = Vec::with_capacity(installations.len());

            for installation in installations {
                let is_active = active_request
                    .as_ref()
                    .is_some_and(|request| installation.matches_request(request));
                let active = if is_active {
                    style("active", StatusTone::Success, color)
                } else {
                    style("installed", StatusTone::Dim, color)
                };
                rows.push(vec![
                    active,
                    installation.vendor.to_string(),
                    installation.requested_version,
                    installation.release_name,
                    display_path(&installation.java_home),
                ]);
            }

            print_status_stdout("java", StatusTone::Info, "installed toolchains");
            print_sharp_table(&["status", "vendor", "request", "release", "home"], &rows);
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
                format!("updated {}", display_path(&config_path)),
            );
        }
    }

    Ok(())
}
