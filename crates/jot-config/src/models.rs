use std::collections::BTreeMap;
use std::path::PathBuf;

use jot_toolchain::{JavaToolchainRequest, KotlinToolchainRequest};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum JavaFormatStyle {
    #[default]
    Google,
    Aosp,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormatConfig {
    pub java_style: JavaFormatStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LintConfig {
    pub pmd_ruleset: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BenchConfig {
    /// Override the JMH version pinned in jot. If `None`, jot's default is used.
    pub jmh_version: Option<String>,
    pub source_dirs: Vec<PathBuf>,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublishConfig {
    pub license: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub scm: Option<String>,
    pub developer: Option<PublishDeveloper>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishDeveloper {
    pub name: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBuildConfig {
    pub config_path: PathBuf,
    pub project_root: PathBuf,
    pub name: String,
    pub version: String,
    pub group: Option<String>,
    pub module_name: Option<String>,
    pub main_class: Option<String>,
    pub source_dirs: Vec<PathBuf>,
    pub test_source_dirs: Vec<PathBuf>,
    pub resource_dir: PathBuf,
    pub dependencies: Vec<String>,
    pub path_dependencies: Vec<PathBuf>,
    pub test_dependencies: Vec<String>,
    pub processors: Vec<String>,
    pub processor_options: BTreeMap<String, String>,
    pub toolchain: Option<JavaToolchainRequest>,
    pub kotlin_toolchain: Option<KotlinToolchainRequest>,
    pub publish: Option<PublishConfig>,
    pub format: FormatConfig,
    pub lint: LintConfig,
    pub bench: BenchConfig,
}

impl ProjectBuildConfig {
    /// Collect all source and test source files with the given extension.
    pub fn source_files_by_ext(&self, ext: &str) -> Vec<PathBuf> {
        let dirs: Vec<PathBuf> = self
            .source_dirs
            .iter()
            .chain(self.test_source_dirs.iter())
            .cloned()
            .collect();
        jot_common::collect_files_by_ext(&dirs, ext)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBuildConfig {
    pub config_path: PathBuf,
    pub root_dir: PathBuf,
    pub group: Option<String>,
    pub toolchain: Option<JavaToolchainRequest>,
    pub kotlin_toolchain: Option<KotlinToolchainRequest>,
    pub members: Vec<WorkspaceMemberBuildConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemberBuildConfig {
    pub module_name: String,
    pub project: ProjectBuildConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDependencySet {
    pub root_dir: PathBuf,
    pub members: Vec<WorkspaceMemberDependencies>,
    pub external_dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemberDependencies {
    pub module_name: String,
    pub project_root: PathBuf,
    pub path_dependencies: Vec<PathBuf>,
    pub external_dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceInheritance {
    pub(crate) group: Option<String>,
    pub(crate) toolchain: Option<JavaToolchainRequest>,
    pub(crate) kotlin_toolchain: Option<KotlinToolchainRequest>,
    pub(crate) module_name: Option<String>,
    pub(crate) catalog_path: Option<PathBuf>,
    pub(crate) publish: Option<PublishConfig>,
    pub(crate) format: Option<FormatConfig>,
    pub(crate) lint: Option<LintConfig>,
}
