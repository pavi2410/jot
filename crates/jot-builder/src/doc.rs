use std::path::{Path, PathBuf};
use std::process::Command;

use jot_config::ProjectBuildConfig;
use jot_resolver::MavenResolver;
use jot_toolchain::InstalledJdk;
use serde_json::{Value, json};

use crate::errors::BuildError;

const DOKKA_CLI: &str = "org.jetbrains.dokka:dokka-cli:2.2.0";
const DOKKA_PLUGINS: &[&str] = &[
    "org.jetbrains.dokka:dokka-base:2.2.0",
    "org.jetbrains.dokka:analysis-kotlin-descriptors:2.2.0",
    "org.jetbrains.kotlinx:kotlinx-html-jvm:0.8.0",
    "org.freemarker:freemarker:2.3.31",
];

pub(crate) fn run_dokka(
    resolver: &MavenResolver,
    jdk: &InstalledJdk,
    project: &ProjectBuildConfig,
    classpath: &[PathBuf],
    docs_dir: &Path,
) -> Result<(), BuildError> {
    // 1. Resolve Dokka CLI and plugin JARs
    let cli_jar = {
        let resolved = resolver.resolve_coordinate(DOKKA_CLI)?;
        resolver.cache_artifact(&resolved.as_coordinate())?
    };
    let plugin_jars: Vec<PathBuf> = DOKKA_PLUGINS
        .iter()
        .map(|coord| {
            let resolved = resolver.resolve_coordinate(coord)?;
            resolver.cache_artifact(&resolved.as_coordinate())
        })
        .collect::<Result<_, _>>()?;

    // 2. Collect source roots that exist
    let source_roots: Vec<&PathBuf> = project.source_dirs.iter().filter(|d| d.exists()).collect();
    if source_roots.is_empty() {
        return Err(BuildError::NoSources(project.project_root.clone()));
    }

    // 3. Write Dokka JSON config
    let config = build_config(project, &source_roots, classpath, &plugin_jars, docs_dir);
    let config_path = project
        .project_root
        .join("target")
        .join("dokka-config.json");
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&config).map_err(|e| BuildError::CommandFailed {
            tool: "dokka",
            stderr: e.to_string(),
        })?,
    )?;

    // 4. Run Dokka (suppress verbose progress output; show stderr only on failure)
    let output = Command::new(jdk.java_binary())
        .current_dir(&project.project_root)
        .arg("-jar")
        .arg(&cli_jar)
        .arg(&config_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let combined = if stderr.is_empty() { stdout } else { stderr };
        return Err(BuildError::CommandFailed {
            tool: "dokka",
            stderr: combined,
        });
    }

    Ok(())
}

fn build_config(
    project: &ProjectBuildConfig,
    source_roots: &[&PathBuf],
    classpath: &[PathBuf],
    plugin_jars: &[PathBuf],
    docs_dir: &Path,
) -> Value {
    let source_root_values: Vec<Value> = source_roots
        .iter()
        .map(|p| Value::String(p.to_string_lossy().into_owned()))
        .collect();

    let classpath_values: Vec<Value> = classpath
        .iter()
        .map(|p| Value::String(p.to_string_lossy().into_owned()))
        .collect();

    let plugin_values: Vec<Value> = plugin_jars
        .iter()
        .map(|p| Value::String(p.to_string_lossy().into_owned()))
        .collect();

    json!({
        "outputDir": docs_dir.to_string_lossy(),
        "moduleName": project.name,
        "moduleVersion": project.version,
        "sourceSets": [
            {
                "sourceSetID": {
                    "scopeId": project.name,
                    "sourceSetName": "main"
                },
                "sourceRoots": source_root_values,
                "classpath": classpath_values
            }
        ],
        "pluginsClasspath": plugin_values
    })
}
