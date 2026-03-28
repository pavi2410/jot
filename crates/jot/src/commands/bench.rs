use jot_builder::JavaProjectBuilder;
use jot_cache::JotPaths;
use jot_resolver::MavenResolver;
use jot_toolchain::ToolchainManager;

use crate::commands::render::{StatusTone, print_status_stdout};

pub(crate) fn handle_bench(
    paths: JotPaths,
    manager: ToolchainManager,
    module: Option<&str>,
    filter: Option<&str>,
    forks: u32,
    warmup: Option<u32>,
    iterations: Option<u32>,
) -> Result<(), anyhow::Error> {
    let resolver = MavenResolver::new(paths)?;
    let builder = JavaProjectBuilder::new(resolver, manager);
    let targets = super::select_project_targets(module)?;
    let roots = match targets {
        super::ProjectTargets::Workspace { roots } => roots,
        super::ProjectTargets::Single { root } => vec![root],
    };

    for project_root in roots {
        let output = builder.bench(&project_root, filter, forks, warmup, iterations)?;
        if output.benchmarks_found {
            print_status_stdout(
                "bench",
                StatusTone::Success,
                format!("completed for {}", output.project.name),
            );
        } else {
            print_status_stdout(
                "bench",
                StatusTone::Dim,
                format!("no benchmark sources found for {}", output.project.name),
            );
        }
    }
    Ok(())
}
