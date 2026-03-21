use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jot_config::{ProjectBuildConfig, load_project_build_config};
use jot_resolver::{MavenResolver, ResolvedArtifact};
use jot_toolchain::{InstalledJdk, ToolchainManager};

const DEFAULT_RESOLVE_DEPTH: usize = 8;

#[derive(Debug)]
pub struct JavaProjectBuilder {
	resolver: MavenResolver,
	toolchains: ToolchainManager,
}

impl JavaProjectBuilder {
	pub fn new(resolver: MavenResolver, toolchains: ToolchainManager) -> Self {
		Self {
			resolver,
			toolchains,
		}
	}

	pub fn build(&self, start: &Path) -> Result<BuildOutput, BuildError> {
		let project = load_project_build_config(start)?;
		let toolchain_request = project
			.toolchain
			.clone()
			.ok_or_else(|| BuildError::MissingJavaToolchain(project.config_path.clone()))?;
		let installed_jdk = self.toolchains.ensure_installed(&toolchain_request)?;
		let dependencies = self
			.resolver
			.resolve_artifacts(&project.dependencies, DEFAULT_RESOLVE_DEPTH)?;
		let target_dir = project.project_root.join("target");
		let classes_dir = target_dir.join("classes");
		prepare_directory(&classes_dir)?;

		let source_files = collect_java_sources(&project.source_dirs)?;
		if source_files.is_empty() {
			return Err(BuildError::NoJavaSources(project.project_root.clone()));
		}

		compile_sources(&installed_jdk, &project, &dependencies, &classes_dir, &source_files)?;
		copy_resources(&project.resource_dir, &classes_dir)?;
		let jar_path = target_dir.join(format!("{}-{}.jar", project.name, project.version));
		package_jar(&installed_jdk, &classes_dir, &jar_path, project.main_class.as_deref())?;

		Ok(BuildOutput {
			project,
			installed_jdk,
			dependencies,
			classes_dir,
			jar_path,
		})
	}

	pub fn run(&self, start: &Path, args: &[String]) -> Result<BuildOutput, BuildError> {
		let output = self.build(start)?;
		let main_class = output
			.project
			.main_class
			.clone()
			.ok_or_else(|| BuildError::MissingMainClass(output.project.config_path.clone()))?;
		let mut classpath_entries = vec![output.classes_dir.clone()];
		classpath_entries.extend(output.dependencies.iter().map(|artifact| artifact.path.clone()));
		let classpath = join_paths_for_classpath(&classpath_entries)?;

		let status = Command::new(output.installed_jdk.java_binary())
			.current_dir(&output.project.project_root)
			.arg("-cp")
			.arg(classpath)
			.arg(main_class)
			.args(args)
			.stdin(Stdio::inherit())
			.stdout(Stdio::inherit())
			.stderr(Stdio::inherit())
			.status()?;

		if !status.success() {
			return Err(BuildError::ProcessExit {
				tool: "java",
				code: status.code(),
			});
		}

		Ok(output)
	}
}

#[derive(Debug)]
pub struct BuildOutput {
	pub project: ProjectBuildConfig,
	pub installed_jdk: InstalledJdk,
	pub dependencies: Vec<ResolvedArtifact>,
	pub classes_dir: PathBuf,
	pub jar_path: PathBuf,
}

fn compile_sources(
	installed_jdk: &InstalledJdk,
	project: &ProjectBuildConfig,
	dependencies: &[ResolvedArtifact],
	classes_dir: &Path,
	source_files: &[PathBuf],
) -> Result<(), BuildError> {
	let mut command = Command::new(installed_jdk.javac_binary());
	command
		.current_dir(&project.project_root)
		.arg("-d")
		.arg(classes_dir);

	if !dependencies.is_empty() {
		let dependency_paths = dependencies
			.iter()
			.map(|artifact| artifact.path.clone())
			.collect::<Vec<_>>();
		command
			.arg("-classpath")
			.arg(join_paths_for_classpath(&dependency_paths)?);
	}

	if let Some(release) = java_release_flag(
		&project
			.toolchain
			.as_ref()
			.map(|toolchain| toolchain.version.as_str())
			.unwrap_or_default(),
	) {
		command.arg("--release").arg(release);
	}

	command.args(source_files);
	let output = command.output()?;
	if !output.status.success() {
		return Err(BuildError::CommandFailed {
			tool: "javac",
			stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
		});
	}

	Ok(())
}

fn package_jar(
	installed_jdk: &InstalledJdk,
	classes_dir: &Path,
	jar_path: &Path,
	main_class: Option<&str>,
) -> Result<(), BuildError> {
	if let Some(parent) = jar_path.parent() {
		fs::create_dir_all(parent)?;
	}

	let mut command = Command::new(installed_jdk.jar_binary());
	command.arg("--create").arg("--file").arg(jar_path);
	if let Some(main_class) = main_class {
		command.arg("--main-class").arg(main_class);
	}
	command.arg("-C").arg(classes_dir).arg(".");

	let output = command.output()?;
	if !output.status.success() {
		return Err(BuildError::CommandFailed {
			tool: "jar",
			stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
		});
	}

	Ok(())
}

fn prepare_directory(path: &Path) -> Result<(), BuildError> {
	if path.exists() {
		fs::remove_dir_all(path)?;
	}
	fs::create_dir_all(path)?;
	Ok(())
}

fn collect_java_sources(source_dirs: &[PathBuf]) -> Result<Vec<PathBuf>, BuildError> {
	let mut files = Vec::new();
	for source_dir in source_dirs {
		collect_java_sources_in_dir(source_dir, &mut files)?;
	}
	files.sort();
	Ok(files)
}

fn collect_java_sources_in_dir(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), BuildError> {
	if !path.exists() {
		return Ok(());
	}

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let entry_path = entry.path();
		if entry.file_type()?.is_dir() {
			collect_java_sources_in_dir(&entry_path, files)?;
		} else if entry_path.extension().and_then(|value| value.to_str()) == Some("java") {
			files.push(entry_path);
		}
	}

	Ok(())
}

fn copy_resources(source: &Path, destination: &Path) -> Result<(), BuildError> {
	if !source.exists() {
		return Ok(());
	}

	copy_directory_contents(source, destination)?;
	Ok(())
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), BuildError> {
	fs::create_dir_all(destination)?;
	for entry in fs::read_dir(source)? {
		let entry = entry?;
		let source_path = entry.path();
		let target_path = destination.join(entry.file_name());
		if entry.file_type()?.is_dir() {
			copy_directory_contents(&source_path, &target_path)?;
		} else {
			if let Some(parent) = target_path.parent() {
				fs::create_dir_all(parent)?;
			}
			fs::copy(&source_path, &target_path)?;
		}
	}
	Ok(())
}

fn join_paths_for_classpath(paths: &[PathBuf]) -> Result<OsString, BuildError> {
	std::env::join_paths(paths).map_err(BuildError::JoinPaths)
}

fn java_release_flag(version: &str) -> Option<String> {
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
	#[error("failed to build classpath: {0}")]
	JoinPaths(#[source] std::env::JoinPathsError),
	#[error("missing [toolchains].java in {0}")]
	MissingJavaToolchain(PathBuf),
	#[error("missing `main-class` in [project] section of {0}")]
	MissingMainClass(PathBuf),
	#[error("no Java source files found under {0}")]
	NoJavaSources(PathBuf),
	#[error("{tool} failed: {stderr}")]
	CommandFailed { tool: &'static str, stderr: String },
	#[error("{tool} exited with status {code:?}")]
	ProcessExit { tool: &'static str, code: Option<i32> },
}

#[cfg(test)]
mod tests {
	use super::{collect_java_sources, java_release_flag};
	use std::fs;
	use tempfile::tempdir;

	#[test]
	fn extracts_major_release_from_toolchain_version() {
		assert_eq!(java_release_flag("21"), Some("21".to_owned()));
		assert_eq!(java_release_flag("17.0.10"), Some("17".to_owned()));
		assert_eq!(java_release_flag("latest"), None);
	}

	#[test]
	fn collects_java_sources_recursively() {
		let temp = tempdir().expect("tempdir");
		let src = temp.path().join("src/main/java/dev/demo");
		fs::create_dir_all(&src).expect("create src");
		fs::write(src.join("Main.java"), "class Main {} ").expect("write java");
		fs::write(src.join("README.txt"), "ignore").expect("write text");

		let sources =
			collect_java_sources(&[temp.path().join("src/main/java")]).expect("collect sources");
		assert_eq!(sources.len(), 1);
		assert!(sources[0].ends_with("Main.java"));
	}
}