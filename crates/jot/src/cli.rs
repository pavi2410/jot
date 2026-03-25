use std::path::PathBuf;

use clap::{Parser, Subcommand};
#[derive(Debug, Parser)]
#[command(name = "jot", version, about = "A JVM toolchain manager")]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    pub offline: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Build {
        #[arg(long)]
        module: Option<String>,
    },
    Publish {
        #[arg(long)]
        module: Option<String>,
        #[arg(long)]
        repository: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        signing_key: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        allow_snapshot: bool,
    },
    Init {
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long = "package")]
        package_name: Option<String>,
        name: Option<String>,
    },
    Fmt {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        module: Option<String>,
    },
    Lint {
        #[arg(long)]
        module: Option<String>,
    },
    Audit {
        #[arg(long)]
        fix: bool,
        #[arg(long)]
        ci: bool,
    },
    Clean {
        #[arg(long)]
        global: bool,
    },
    Add {
        coordinate: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
        #[arg(long)]
        test: bool,
        #[arg(long)]
        name: Option<String>,
    },
    Remove {
        name: String,
        #[arg(long)]
        test: bool,
    },
    Deps {
        #[arg(long)]
        module: Option<String>,
    },
    Outdated {
        #[arg(long)]
        module: Option<String>,
    },
    Lock {
        dependencies: Vec<String>,
        #[arg(long, default_value_t = 8)]
        depth: usize,
        #[arg(long, default_value = "jot.lock")]
        output: PathBuf,
    },
    Resolve {
        dependency: String,
        #[arg(long)]
        deps: bool,
    },
    Tree {
        dependency: Option<String>,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long)]
        workspace: bool,
        #[arg(long)]
        module: Option<String>,
    },
    Run {
        #[arg(long)]
        module: Option<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    Test {
        #[arg(long)]
        module: Option<String>,
    },
    Doc {
        #[arg(long)]
        module: Option<String>,
        /// Open generated docs in the browser after building
        #[arg(long)]
        open: bool,
    },
    #[command(name = "self")]
    SelfCmd(SelfCommand),
    Toolchain(ToolchainCommand),
}

#[derive(Debug, clap::Args)]
pub(crate) struct SelfCommand {
    #[command(subcommand)]
    pub command: SelfSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SelfSubcommand {
    Update {
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        check: bool,
        #[arg(long, short = 'y')]
        yes: bool,
    },
    Uninstall {
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Debug, clap::Args)]
pub(crate) struct ToolchainCommand {
    #[command(subcommand)]
    pub command: ToolchainSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ToolchainSubcommand {
    /// Install a toolchain (e.g. java@21, java@corretto-21, kotlin@2.1.0)
    Install {
        /// Toolchain to install (e.g. java@21, java@corretto-21, kotlin@2.1.0)
        toolchain: String,
        #[arg(long)]
        force: bool,
    },
    /// List installed toolchains
    List,
    /// Pin a toolchain version in jot.toml (e.g. java@21, java@zulu-21, kotlin@2.1.0)
    Pin {
        /// Toolchain to pin (e.g. java@21, java@zulu-21, kotlin@2.1.0)
        toolchain: String,
        #[arg(long)]
        workspace: bool,
    },
}
