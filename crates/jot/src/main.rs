use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jot_cache::JotPaths;
use jot_config::{pin_java_toolchain, read_toolchain_request};
use jot_toolchain::{InstallOptions, JavaToolchainRequest, JdkVendor, ToolchainManager};

#[derive(Debug, Parser)]
#[command(name = "jot", version, about = "A JVM toolchain manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Java(JavaCommand),
}

#[derive(Debug, clap::Args)]
struct JavaCommand {
    #[command(subcommand)]
    command: JavaSubcommand,
}

#[derive(Debug, Subcommand)]
enum JavaSubcommand {
    Install {
        version: String,
        #[arg(long, default_value = "adoptium")]
        vendor: JdkVendor,
        #[arg(long)]
        force: bool,
    },
    List,
    Pin {
        version: String,
        #[arg(long)]
        vendor: Option<JdkVendor>,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let paths = JotPaths::new()?;
    paths.ensure_exists()?;
    let manager = ToolchainManager::new(paths.clone())?;

    match cli.command {
        Command::Java(command) => handle_java(command, manager, paths)?,
    }

    Ok(())
}

fn handle_java(
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
        JavaSubcommand::Pin { version, vendor } => {
            let cwd = std::env::current_dir()?;
            let config_path = nearest_project_file(&cwd)?;
            pin_java_toolchain(
                &config_path,
                &JavaToolchainRequest { version, vendor },
            )?;
            println!("updated {}", config_path.display());
        }
    }

    Ok(())
}

fn nearest_project_file(start: &PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    jot_config::find_jot_toml(start)?.ok_or_else(|| {
        "could not find jot.toml in the current directory or any parent directory".into()
    })
}