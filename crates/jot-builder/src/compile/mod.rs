mod javac;
mod kotlinc;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jot_config::ProjectBuildConfig;
use jot_resolver::MavenResolver;
use jot_toolchain::{InstalledJdk, InstalledKotlin};

use crate::errors::BuildError;

pub(crate) use javac::Javac;
pub(crate) use kotlinc::Kotlinc;

// ── Trait & types ───────────────────────────────────────────────────────────

pub(crate) struct CompileConfig {
    pub project_root: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub jvm_target: Option<String>,
}

pub(crate) struct CompileResult {
    /// Additional classpath entries for subsequent compilers in the pipeline.
    pub extra_classpath: Vec<PathBuf>,
}

pub(crate) trait SourceCompiler {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
    fn collect_sources(&self, source_dirs: &[PathBuf]) -> Result<Vec<PathBuf>, BuildError>;
    fn compile(
        &self,
        config: &CompileConfig,
        sources: &[PathBuf],
    ) -> Result<CompileResult, BuildError>;
}

// ── Pipeline ────────────────────────────────────────────────────────────────

/// Run compilers in order. Each compiler's output extends the classpath for the next.
pub(crate) fn compile_pipeline(
    compilers: &[Box<dyn SourceCompiler>],
    source_dirs: &[PathBuf],
    base_classpath: &[PathBuf],
    output_dir: &Path,
    project_root: &Path,
    jvm_target: Option<&str>,
) -> Result<(), BuildError> {
    let mut classpath = base_classpath.to_vec();

    for compiler in compilers {
        let sources = compiler.collect_sources(source_dirs)?;
        if sources.is_empty() {
            continue;
        }
        let config = CompileConfig {
            project_root: project_root.to_path_buf(),
            classpath: classpath.clone(),
            output_dir: output_dir.to_path_buf(),
            jvm_target: jvm_target.map(|s| s.to_owned()),
        };
        let result = compiler.compile(&config, &sources)?;
        classpath.extend(result.extra_classpath);
    }
    Ok(())
}

// ── Annotation processing config ────────────────────────────────────────────

pub(crate) struct AnnotationProcessingConfig {
    pub processor_paths: Vec<PathBuf>,
    pub options: BTreeMap<String, String>,
    pub generated_sources_dir: PathBuf,
}

/// Resolve annotation processors from project config if any are declared.
pub(crate) fn resolve_annotation_processing(
    project: &ProjectBuildConfig,
    resolver: &MavenResolver,
    target_dir: &Path,
) -> Result<Option<AnnotationProcessingConfig>, BuildError> {
    if project.processors.is_empty() {
        return Ok(None);
    }
    let processor_artifacts =
        resolver.resolve_artifacts(&project.processors, crate::DEFAULT_RESOLVE_DEPTH)?;
    let generated_sources_dir = target_dir.join("generated-sources");
    crate::prepare_directory(&generated_sources_dir)?;
    Ok(Some(AnnotationProcessingConfig {
        processor_paths: processor_artifacts
            .iter()
            .map(|artifact| artifact.path.clone())
            .collect(),
        options: project.processor_options.clone(),
        generated_sources_dir,
    }))
}

// ── Compiler chain builder ──────────────────────────────────────────────────

/// Build the ordered list of compilers: Kotlinc first (if present), then Javac.
pub(crate) fn build_compiler_chain(
    installed_kotlin: Option<&InstalledKotlin>,
    installed_jdk: &InstalledJdk,
    java_source_roots: Option<&[PathBuf]>,
    annotation_processing: Option<AnnotationProcessingConfig>,
) -> Vec<Box<dyn SourceCompiler>> {
    let mut compilers: Vec<Box<dyn SourceCompiler>> = Vec::new();

    if let Some(kotlin) = installed_kotlin {
        compilers.push(Box::new(Kotlinc::new(
            kotlin.clone(),
            java_source_roots.map(|roots| roots.to_vec()),
        )));
    }

    compilers.push(Box::new(Javac::new(
        installed_jdk.clone(),
        annotation_processing,
    )));

    compilers
}

// ── Shared helpers ──────────────────────────────────────────────────────────

pub(crate) fn java_release_flag(version: &str) -> Option<String> {
    let digits = version
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}
