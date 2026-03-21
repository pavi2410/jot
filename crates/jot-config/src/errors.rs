use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid start path: {0}")]
    InvalidStartPath(PathBuf),
    #[error("could not find jot.toml starting from {0}")]
    ProjectConfigNotFound(PathBuf),
    #[error("missing [project] section in {0}")]
    MissingProjectSection(PathBuf),
    #[error("missing [project].{field} in {path}")]
    MissingProjectField { path: PathBuf, field: &'static str },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse jot.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("failed to update jot.toml: {0}")]
    EditToml(#[from] toml_edit::TomlError),
    #[error("unsupported dependency declaration for `{name}`: {reason}")]
    UnsupportedDependencyDeclaration { name: String, reason: String },
    #[error("dependency `{dependency}` uses catalog syntax but no libs.versions.toml was found")]
    MissingCatalogFile { dependency: String },
    #[error("dependency `{dependency}` references missing catalog alias `{alias}`")]
    MissingCatalogEntry { dependency: String, alias: String },
    #[error("dependency `{dependency}` alias `{alias}` references missing version `{version_ref}`")]
    MissingCatalogVersion {
        dependency: String,
        alias: String,
        version_ref: String,
    },
    #[error("missing [workspace] section in {0}")]
    MissingWorkspaceSection(PathBuf),
    #[error("invalid [workspace] config in {path}: {reason}")]
    InvalidWorkspaceConfig { path: PathBuf, reason: String },
    #[error("invalid workspace member path `{0}`")]
    InvalidWorkspaceMember(String),
    #[error("workspace member config not found: {0}")]
    WorkspaceMemberNotFound(PathBuf),
    #[error("duplicate workspace module name `{0}`")]
    DuplicateWorkspaceModule(String),
    #[error("invalid path dependency `{name}` at {path}: {reason}")]
    InvalidPathDependency {
        name: String,
        path: PathBuf,
        reason: String,
    },
    #[error("workspace module `{module}` depends on path outside workspace: {dependency}")]
    PathDependencyOutsideWorkspace { module: String, dependency: PathBuf },
    #[error("workspace path dependency cycle detected: {0}")]
    WorkspacePathDependencyCycle(String),
}
