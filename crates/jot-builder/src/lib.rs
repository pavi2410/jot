mod compile;
mod diagnostics;
pub mod errors;
mod package;
mod workspace;

pub use errors::BuildError;
pub use workspace::{WorkspaceBuildOutput, WorkspaceModuleBuildOutput};

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jot_config::{ProjectBuildConfig, load_project_build_config};
use jot_resolver::{MavenResolver, ResolvedArtifact};
use jot_toolchain::{InstalledJdk, InstalledKotlin, ToolchainManager};

use compile::{
    build_compiler_chain, compile_pipeline, join_paths_for_classpath, resolve_annotation_processing,
};
use package::{build_fat_jar, copy_resources, package_jar};

const DEFAULT_RESOLVE_DEPTH: usize = 8;
const DEFAULT_JUNIT_CONSOLE_COORD: &str =
    "org.junit.platform:junit-platform-console-standalone:6.0.3";

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
        let mut cache = HashMap::<PathBuf, BuildOutput>::new();
        let mut stack = Vec::<PathBuf>::new();
        let project = load_project_build_config(start)?;
        self.build_project_with_cache(project, &mut cache, &mut stack)
    }

    fn build_project_internal(
        &self,
        project: ProjectBuildConfig,
        extra_classpath: &[PathBuf],
    ) -> Result<BuildOutput, BuildError> {
        let toolchain_request = project
            .toolchain
            .clone()
            .ok_or_else(|| BuildError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain_request)?;

        let installed_kotlin = match &project.kotlin_toolchain {
            Some(request) => Some(self.toolchains.ensure_kotlin_installed(request)?),
            None => None,
        };

        let dependencies = self
            .resolver
            .resolve_artifacts(&project.dependencies, DEFAULT_RESOLVE_DEPTH)?;
        let target_dir = project.project_root.join("target");
        let classes_dir = target_dir.join("classes");
        prepare_directory(&classes_dir)?;

        let java_sources = compile::collect_sources_by_extension(&project.source_dirs, "java")?;
        let kotlin_sources = compile::collect_sources_by_extension(&project.source_dirs, "kt")?;
        if java_sources.is_empty() && kotlin_sources.is_empty() {
            return Err(BuildError::NoSources(project.project_root.clone()));
        }

        let mut dependency_paths = dependencies
            .iter()
            .map(|artifact| artifact.path.clone())
            .collect::<Vec<_>>();
        dependency_paths.extend(extra_classpath.iter().cloned());

        if let Some(ref kotlin) = installed_kotlin {
            let stdlib = kotlin.kotlin_stdlib_jar();
            if stdlib.is_file() {
                dependency_paths.push(stdlib);
            }
        }

        let jvm_target = project
            .toolchain
            .as_ref()
            .map(|value| value.version.as_str());

        let annotation_processing =
            resolve_annotation_processing(&project, &self.resolver, &target_dir)?;
        let compilers = build_compiler_chain(
            installed_kotlin.as_ref(),
            &installed_jdk,
            Some(project.source_dirs.as_slice()),
            annotation_processing,
        );
        compile_pipeline(
            &compilers,
            &project.source_dirs,
            &dependency_paths,
            &classes_dir,
            &project.project_root,
            jvm_target,
        )?;

        copy_resources(&project.resource_dir, &classes_dir)?;
        let jar_path = target_dir.join(format!("{}-{}.jar", project.name, project.version));
        package_jar(&installed_jdk, &classes_dir, &jar_path, None)?;

        let (fat_jar_path, fat_jar_warnings) =
            if let Some(main_class) = project.main_class.as_deref() {
                let path = target_dir.join("bin").join(format!("{}.jar", project.name));
                let mut fat_jar_dependencies = dependencies
                    .iter()
                    .map(|item| item.path.clone())
                    .collect::<Vec<_>>();
                fat_jar_dependencies.extend(extra_classpath.iter().cloned());
                if let Some(ref kotlin) = installed_kotlin {
                    let stdlib = kotlin.kotlin_stdlib_jar();
                    if stdlib.is_file() {
                        fat_jar_dependencies.push(stdlib);
                    }
                }
                let warnings = build_fat_jar(
                    &installed_jdk,
                    &fat_jar_dependencies,
                    &classes_dir,
                    &path,
                    main_class,
                )?;
                (Some(path), warnings)
            } else {
                (None, Vec::new())
            };

        Ok(BuildOutput {
            project,
            installed_jdk,
            installed_kotlin,
            dependencies,
            classes_dir,
            jar_path,
            fat_jar_path,
            fat_jar_warnings,
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
        classpath_entries.extend(
            output
                .dependencies
                .iter()
                .map(|artifact| artifact.path.clone()),
        );
        if let Some(ref kotlin) = output.installed_kotlin {
            let stdlib = kotlin.kotlin_stdlib_jar();
            if stdlib.is_file() {
                classpath_entries.push(stdlib);
            }
        }
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

    pub fn test(&self, start: &Path) -> Result<TestOutput, BuildError> {
        let project = load_project_build_config(start)?;
        let mut cache = HashMap::<PathBuf, BuildOutput>::new();
        let mut stack = Vec::<PathBuf>::new();
        let mut path_dependency_jars = Vec::new();
        for dependency_root in &project.path_dependencies {
            let dependency_project = load_project_build_config(dependency_root)?;
            let dependency_output =
                self.build_project_with_cache(dependency_project, &mut cache, &mut stack)?;
            path_dependency_jars.push(dependency_output.jar_path.clone());
        }

        let toolchain_request = project
            .toolchain
            .clone()
            .ok_or_else(|| BuildError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain_request)?;

        let installed_kotlin = match &project.kotlin_toolchain {
            Some(request) => Some(self.toolchains.ensure_kotlin_installed(request)?),
            None => None,
        };

        let compile_dependencies = self
            .resolver
            .resolve_artifacts(&project.dependencies, DEFAULT_RESOLVE_DEPTH)?;

        let mut test_dependency_inputs = project.test_dependencies.clone();
        test_dependency_inputs.push(DEFAULT_JUNIT_CONSOLE_COORD.to_owned());
        let test_dependencies = self
            .resolver
            .resolve_artifacts(&test_dependency_inputs, DEFAULT_RESOLVE_DEPTH)?;

        let target_dir = project.project_root.join("target");
        let classes_dir = target_dir.join("classes");
        prepare_directory(&classes_dir)?;

        let main_java_sources =
            compile::collect_sources_by_extension(&project.source_dirs, "java")?;
        let main_kotlin_sources =
            compile::collect_sources_by_extension(&project.source_dirs, "kt")?;

        let jvm_target = project
            .toolchain
            .as_ref()
            .map(|value| value.version.as_str());

        if !main_java_sources.is_empty() || !main_kotlin_sources.is_empty() {
            let mut main_compile_classpath = compile_dependencies
                .iter()
                .map(|item| item.path.clone())
                .collect::<Vec<_>>();
            main_compile_classpath.extend(path_dependency_jars.iter().cloned());

            if let Some(ref kotlin) = installed_kotlin {
                let stdlib = kotlin.kotlin_stdlib_jar();
                if stdlib.is_file() {
                    main_compile_classpath.push(stdlib);
                }
            }

            let annotation_processing =
                resolve_annotation_processing(&project, &self.resolver, &target_dir)?;
            let main_compilers = build_compiler_chain(
                installed_kotlin.as_ref(),
                &installed_jdk,
                Some(project.source_dirs.as_slice()),
                annotation_processing,
            );
            compile_pipeline(
                &main_compilers,
                &project.source_dirs,
                &main_compile_classpath,
                &classes_dir,
                &project.project_root,
                jvm_target,
            )?;

            copy_resources(&project.resource_dir, &classes_dir)?;
        }

        let test_java_sources =
            compile::collect_sources_by_extension(&project.test_source_dirs, "java")?;
        let test_kotlin_sources =
            compile::collect_sources_by_extension(&project.test_source_dirs, "kt")?;

        if test_java_sources.is_empty() && test_kotlin_sources.is_empty() {
            return Ok(TestOutput {
                project,
                tests_found: false,
            });
        }

        let test_classes_dir = target_dir.join("test-classes");
        prepare_directory(&test_classes_dir)?;
        let mut test_compile_classpath = vec![classes_dir.clone()];
        test_compile_classpath.extend(compile_dependencies.iter().map(|item| item.path.clone()));
        test_compile_classpath.extend(path_dependency_jars.iter().cloned());
        test_compile_classpath.extend(test_dependencies.iter().map(|item| item.path.clone()));

        if let Some(ref kotlin) = installed_kotlin {
            let stdlib = kotlin.kotlin_stdlib_jar();
            if stdlib.is_file() && !test_compile_classpath.contains(&stdlib) {
                test_compile_classpath.push(stdlib);
            }
        }

        // Test sources: no annotation processing
        let test_compilers = build_compiler_chain(
            installed_kotlin.as_ref(),
            &installed_jdk,
            Some(project.test_source_dirs.as_slice()),
            None,
        );
        compile_pipeline(
            &test_compilers,
            &project.test_source_dirs,
            &test_compile_classpath,
            &test_classes_dir,
            &project.project_root,
            jvm_target,
        )?;

        let console_jar = test_dependencies
            .iter()
            .find(|item| {
                item.coordinate.group == "org.junit.platform"
                    && item.coordinate.artifact == "junit-platform-console-standalone"
            })
            .map(|item| item.path.clone())
            .ok_or(BuildError::MissingJUnitConsole)?;

        let mut runtime_classpath = vec![classes_dir, test_classes_dir];
        runtime_classpath.extend(compile_dependencies.iter().map(|item| item.path.clone()));
        runtime_classpath.extend(path_dependency_jars);
        runtime_classpath.extend(test_dependencies.iter().map(|item| item.path.clone()));
        if let Some(ref kotlin) = installed_kotlin {
            let stdlib = kotlin.kotlin_stdlib_jar();
            if stdlib.is_file() && !runtime_classpath.contains(&stdlib) {
                runtime_classpath.push(stdlib);
            }
        }
        let classpath = join_paths_for_classpath(&runtime_classpath)?;

        let status = Command::new(installed_jdk.java_binary())
            .current_dir(&project.project_root)
            .arg("-jar")
            .arg(console_jar)
            .arg("execute")
            .arg("--scan-class-path")
            .arg("--class-path")
            .arg(classpath)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if !status.success() {
            return Err(BuildError::ProcessExit {
                tool: "junit",
                code: status.code(),
            });
        }

        Ok(TestOutput {
            project,
            tests_found: true,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BuildOutput {
    pub project: ProjectBuildConfig,
    pub installed_jdk: InstalledJdk,
    pub installed_kotlin: Option<InstalledKotlin>,
    pub dependencies: Vec<ResolvedArtifact>,
    pub classes_dir: PathBuf,
    pub jar_path: PathBuf,
    pub fat_jar_path: Option<PathBuf>,
    pub fat_jar_warnings: Vec<String>,
}

#[derive(Debug)]
pub struct TestOutput {
    pub project: ProjectBuildConfig,
    pub tests_found: bool,
}

fn prepare_directory(path: &Path) -> Result<(), BuildError> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compile::collect_sources_by_extension;
    use super::compile::java_release_flag;
    use super::diagnostics::{caret_span, format_javac_stderr, parse_javac_diagnostics};
    use super::package::merge_service_contents;
    use std::collections::BTreeMap;
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

        let sources = collect_sources_by_extension(&[temp.path().join("src/main/java")], "java")
            .expect("collect sources");
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
        assert!(formatted.contains("error: ';' expected"));
        assert!(formatted.contains("src/main/java/demo/Main.java"));
        assert!(formatted.contains("^"));
        assert!(!formatted.contains("\u{1b}["));
    }

    #[test]
    fn maps_caret_line_to_span() {
        assert_eq!(caret_span("   ^^"), Some((3, 5)));
        assert_eq!(caret_span("  ^"), Some((2, 3)));
        assert_eq!(caret_span("none"), None);
    }

    #[test]
    fn merges_service_lines_without_duplicates() {
        let mut services = BTreeMap::new();
        merge_service_contents(
            &mut services,
            "META-INF/services/demo.Service",
            b"a.Provider\nb.Provider\n",
        );
        merge_service_contents(
            &mut services,
            "META-INF/services/demo.Service",
            b"b.Provider\nc.Provider\n",
        );
        assert_eq!(
            services.get("META-INF/services/demo.Service").cloned(),
            Some(vec![
                "a.Provider".to_owned(),
                "b.Provider".to_owned(),
                "c.Provider".to_owned()
            ])
        );
    }
}
