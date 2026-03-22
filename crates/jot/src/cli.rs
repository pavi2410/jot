use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jot_toolchain::JdkVendor;

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
    #[command(name = "self")]
    SelfCmd(SelfCommand),
    Java(JavaCommand),
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
pub(crate) struct JavaCommand {
    #[command(subcommand)]
    pub command: JavaSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum JavaSubcommand {
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
        #[arg(long)]
        workspace: bool,
    },
}
