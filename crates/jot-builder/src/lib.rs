use std::ffi::OsString;
use std::fs;
use std::io::IsTerminal;
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
		let raw_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
		return Err(BuildError::CommandFailed {
			tool: "javac",
			stderr: format_javac_stderr(&raw_stderr, std::io::stderr().is_terminal()),
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

fn format_javac_stderr(raw: &str, color: bool) -> String {
	let diagnostics = parse_javac_diagnostics(raw);
	if diagnostics.is_empty() {
		return raw.to_owned();
	}

	let mut output = String::new();
	output.push_str(&style("javac diagnostics", Style::Bold, color));
	output.push('\n');

	for diagnostic in diagnostics {
		let severity_style = match diagnostic.severity {
			DiagnosticSeverity::Error => Style::RedBold,
			DiagnosticSeverity::Warning => Style::YellowBold,
		};
		output.push_str(&format!(
			"{} {}:{}\n",
			style(diagnostic.severity.as_str(), severity_style, color),
			style(&diagnostic.path, Style::Cyan, color),
			diagnostic.line
		));
		output.push_str(&format!("  {}\n", diagnostic.message));
		if let Some(source_line) = diagnostic.source_line.as_ref() {
			output.push_str(&format!("  {}\n", source_line));
		}
		if let Some(caret_line) = diagnostic.caret_line.as_ref() {
			output.push_str(&format!("  {}\n", style(caret_line, Style::Green, color)));
		}
	}

	output.trim_end().to_owned()
}

fn parse_javac_diagnostics(raw: &str) -> Vec<JavacDiagnostic> {
	let mut diagnostics = Vec::new();
	let lines = raw.lines().collect::<Vec<_>>();
	let mut index = 0;

	while index < lines.len() {
		let line = lines[index].trim_end();
		if let Some((path, line_number, severity, message)) = parse_diagnostic_header(line) {
			let mut source_line = None;
			let mut caret_line = None;

			if index + 1 < lines.len() {
				let candidate = lines[index + 1].trim_end();
				if !candidate.contains(": error:") && !candidate.contains(": warning:") {
					source_line = Some(candidate.to_owned());
					index += 1;

					if index + 1 < lines.len() {
						let caret_candidate = lines[index + 1].trim_end();
						if caret_candidate.contains('^') {
							caret_line = Some(caret_candidate.to_owned());
							index += 1;
						}
					}
				}
			}

			diagnostics.push(JavacDiagnostic {
				path,
				line: line_number,
				severity,
				message,
				source_line,
				caret_line,
			});
		}

		index += 1;
	}

	diagnostics
}

fn parse_diagnostic_header(
	line: &str,
) -> Option<(String, usize, DiagnosticSeverity, String)> {
	let (severity, marker) = if line.contains(": error: ") {
		(DiagnosticSeverity::Error, ": error: ")
	} else if line.contains(": warning: ") {
		(DiagnosticSeverity::Warning, ": warning: ")
	} else {
		return None;
	};

	let marker_idx = line.find(marker)?;
	let location = &line[..marker_idx];
	let message = line[marker_idx + marker.len()..].trim().to_owned();
	let split_idx = location.rfind(':')?;
	let path = location[..split_idx].to_owned();
	let line_number = location[split_idx + 1..].parse::<usize>().ok()?;

	Some((path, line_number, severity, message))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticSeverity {
	Error,
	Warning,
}

impl DiagnosticSeverity {
	fn as_str(self) -> &'static str {
		match self {
			Self::Error => "error",
			Self::Warning => "warning",
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JavacDiagnostic {
	path: String,
	line: usize,
	severity: DiagnosticSeverity,
	message: String,
	source_line: Option<String>,
	caret_line: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
	Bold,
	RedBold,
	YellowBold,
	Cyan,
	Green,
}

fn style(value: &str, style: Style, color: bool) -> String {
	if !color {
		return value.to_owned();
	}

	let code = match style {
		Style::Bold => "1",
		Style::RedBold => "1;31",
		Style::YellowBold => "1;33",
		Style::Cyan => "36",
		Style::Green => "32",
	};
	format!("\x1b[{code}m{value}\x1b[0m")
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
	use super::{
		collect_java_sources, format_javac_stderr, java_release_flag, parse_javac_diagnostics,
	};
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

	#[test]
	fn parses_javac_error_and_warning_diagnostics() {
		let raw = "src/main/java/demo/Main.java:7: error: ';' expected\n        System.out.println(\"oops\")\n                                  ^\nsrc/main/java/demo/Main.java:8: warning: [deprecation] stop() in Thread has been deprecated\n        t.stop();\n          ^\n";
		let diagnostics = parse_javac_diagnostics(raw);
		assert_eq!(diagnostics.len(), 2);
		assert_eq!(diagnostics[0].line, 7);
		assert!(diagnostics[0].message.contains("';' expected"));
		assert_eq!(diagnostics[1].line, 8);
		assert!(diagnostics[1].message.contains("deprecated"));
	}

	#[test]
	fn formats_javac_diagnostics_without_ansi_when_disabled() {
		let raw = "src/main/java/demo/Main.java:7: error: ';' expected\n        System.out.println(\"oops\")\n                                  ^\n";
		let formatted = format_javac_stderr(raw, false);
		assert!(formatted.contains("javac diagnostics"));
		assert!(formatted.contains("error src/main/java/demo/Main.java:7"));
		assert!(formatted.contains("';' expected"));
		assert!(!formatted.contains("\u{1b}["));
	}
}