use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JavacDiagnostic {
    pub path: String,
    pub line: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source_line: Option<String>,
    pub caret_line: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("config error: {0}")]
    Config(#[from] jot_config::ConfigError),
    #[error("resolver error: {0}")]
    Resolver(#[from] jot_resolver::ResolverError),
    #[error("toolchain error: {0}")]
    Toolchain(#[from] jot_toolchain::ToolchainError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("failed to build classpath: {0}")]
    JoinPaths(#[source] std::env::JoinPathsError),
    #[error("invalid fat-jar output path: {0}")]
    InvalidFatJarPath(PathBuf),
    #[error("failed to compute relative path for {0}")]
    StripPrefix(PathBuf),
    #[error("missing [toolchains].java in {0}")]
    MissingJavaToolchain(PathBuf),
    #[error("missing `main-class` in [project] section of {0}")]
    MissingMainClass(PathBuf),
    #[error("workspace config not found from {0}")]
    WorkspaceNotFound(PathBuf),
    #[error("unknown workspace module `{0}`")]
    UnknownWorkspaceModule(String),
    #[error("module `{module}` has unknown workspace path dependency {path}")]
    UnknownWorkspaceDependency { module: String, path: PathBuf },
    #[error("workspace dependency graph contains a cycle")]
    WorkspaceCycleDetected,
    #[error("path dependency cycle detected: {0}")]
    PathDependencyCycle(String),
    #[error("no Java source files found under {0}")]
    NoJavaSources(PathBuf),
    #[error("could not locate junit-platform-console-standalone in resolved test dependencies")]
    MissingJUnitConsole,
    #[error("{tool} failed: {stderr}")]
    CommandFailed { tool: &'static str, stderr: String },
    #[error("{tool} exited with status {code:?}")]
    ProcessExit {
        tool: &'static str,
        code: Option<i32>,
    },
}
