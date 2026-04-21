mod compile;
mod diagnostics;
mod doc;
pub mod errors;
mod graph;
mod manifest;
mod package;
mod workspace;

pub use errors::BuildError;
pub use workspace::{WorkspaceBuildOutput, WorkspaceModuleBuildOutput};

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jot_config::{ProjectBuildConfig, load_project_build_config};
use jot_resolver::{MavenResolver, ResolvedArtifact};
use jot_toolchain::{InstalledJdk, InstalledKotlin, ToolchainManager};

use compile::{
    AnnotationProcessingConfig, build_compiler_chain, compile_pipeline,
    resolve_annotation_processing,
};
use package::{build_fat_jar, copy_resources, package_jar};

const DEFAULT_RESOLVE_DEPTH: usize = 8;
const DEFAULT_JUNIT_CONSOLE_COORD: &str =
    "org.junit.platform:junit-platform-console-standalone:6.0.3";
const DEFAULT_JMH_VERSION: &str = "1.37";

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

        let java_sources = jot_common::collect_files_by_ext(&project.source_dirs, "java");
        let kotlin_sources = jot_common::collect_files_by_ext(&project.source_dirs, "kt");
        if java_sources.is_empty() && kotlin_sources.is_empty() {
            return Err(BuildError::NoSources(project.project_root.clone()));
        }

        // Combine all sources for incremental tracking.
        let all_sources: Vec<PathBuf> = {
            let mut v = java_sources;
            v.extend(kotlin_sources);
            v
        };

        let dependency_paths = ClasspathAssembler::new()
            .with_artifacts(&dependencies)
            .with_paths(extra_classpath.iter().cloned())
            .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
            .build();

        let jvm_target = project
            .toolchain
            .as_ref()
            .map(|value| value.version.as_str());

        let annotation_processing =
            resolve_annotation_processing(&project, &self.resolver, &target_dir)?;

        // Compute hashes for the toolchain and full classpath (deps + processors)
        // before moving annotation_processing into the compiler chain.
        let kotlin_version = project.kotlin_toolchain.as_ref().map(|tc| tc.version.as_str());
        let toolchain_hash =
            manifest::compute_toolchain_hash(&toolchain_request.version, kotlin_version);
        let mut hash_paths = dependency_paths.clone();
        if let Some(ap) = &annotation_processing {
            hash_paths.extend_from_slice(&ap.processor_paths);
        }
        let classpath_hash = manifest::compute_classpath_hash(&hash_paths);

        let compilers = build_compiler_chain(
            installed_kotlin.as_ref(),
            &installed_jdk,
            Some(project.source_dirs.as_slice()),
            annotation_processing,
        );

        let manifest_path = target_dir.join(manifest::MANIFEST_FILENAME);
        let existing_manifest = manifest::BuildManifest::load(&manifest_path);
        let status = manifest::classify_sources(
            existing_manifest.as_ref(),
            &toolchain_hash,
            &classpath_hash,
            &all_sources,
        )?;

        let classes_rebuilt = match &status {
            manifest::IncrementalStatus::FullRebuild { .. } => {
                prepare_directory(&classes_dir)?;
                compile_pipeline(
                    &compilers,
                    &project.source_dirs,
                    &dependency_paths,
                    &classes_dir,
                    &project.project_root,
                    jvm_target,
                    None,
                )?;
                true
            }
            manifest::IncrementalStatus::Incremental { dirty } => {
                ensure_directory(&classes_dir)?;
                let dirty_set: HashSet<PathBuf> = dirty.iter().cloned().collect();
                compile_pipeline(
                    &compilers,
                    &project.source_dirs,
                    &dependency_paths,
                    &classes_dir,
                    &project.project_root,
                    jvm_target,
                    Some(&dirty_set),
                )?;
                true
            }
            manifest::IncrementalStatus::UpToDate => {
                ensure_directory(&classes_dir)?;
                false
            }
        };

        copy_resources(&project.resource_dir, &classes_dir)?;

        // Persist the updated manifest after a successful compilation.
        if classes_rebuilt {
            try_save_manifest(&manifest_path, toolchain_hash, classpath_hash, &all_sources);
        }

        let jar_path = target_dir.join(format!("{}-{}.jar", project.name, project.version));
        // Re-package the JAR whenever sources were recompiled, or if the JAR is missing.
        if classes_rebuilt || !jar_path.exists() {
            package_jar(&installed_jdk, &classes_dir, &jar_path, None)?;
        }

        let (fat_jar_path, fat_jar_warnings) =
            if let Some(main_class) = project.main_class.as_deref() {
                let path = target_dir.join("bin").join(format!("{}.jar", project.name));
                let fat_jar_dependencies = ClasspathAssembler::new()
                    .with_artifacts(&dependencies)
                    .with_paths(extra_classpath.iter().cloned())
                    .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
                    .build();
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
        let classpath_entries = ClasspathAssembler::new()
            .with_paths([output.classes_dir.clone()])
            .with_artifacts(&output.dependencies)
            .with_optional_unique_path(kotlin_stdlib_jar(output.installed_kotlin.as_ref()))
            .build();
        let classpath = std::env::join_paths(&classpath_entries)?;

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

        let main_java_sources = jot_common::collect_files_by_ext(&project.source_dirs, "java");
        let main_kotlin_sources = jot_common::collect_files_by_ext(&project.source_dirs, "kt");

        let jvm_target = project
            .toolchain
            .as_ref()
            .map(|value| value.version.as_str());

        let kotlin_version = project.kotlin_toolchain.as_ref().map(|tc| tc.version.as_str());
        let toolchain_hash =
            manifest::compute_toolchain_hash(&toolchain_request.version, kotlin_version);

        if !main_java_sources.is_empty() || !main_kotlin_sources.is_empty() {
            let main_compile_classpath = ClasspathAssembler::new()
                .with_artifacts(&compile_dependencies)
                .with_paths(path_dependency_jars.iter().cloned())
                .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
                .build();

            let annotation_processing =
                resolve_annotation_processing(&project, &self.resolver, &target_dir)?;
            let mut hash_paths = main_compile_classpath.clone();
            if let Some(ap) = &annotation_processing {
                hash_paths.extend_from_slice(&ap.processor_paths);
            }
            let main_classpath_hash = manifest::compute_classpath_hash(&hash_paths);
            let main_manifest_path = target_dir.join(manifest::MANIFEST_FILENAME);
            let main_existing = manifest::BuildManifest::load(&main_manifest_path);

            let all_main_sources: Vec<PathBuf> = {
                let mut v = main_java_sources;
                v.extend(main_kotlin_sources);
                v
            };
            let main_status = manifest::classify_sources(
                main_existing.as_ref(),
                &toolchain_hash,
                &main_classpath_hash,
                &all_main_sources,
            )?;

            let main_compilers = build_compiler_chain(
                installed_kotlin.as_ref(),
                &installed_jdk,
                Some(project.source_dirs.as_slice()),
                annotation_processing,
            );

            let main_compiled = match &main_status {
                manifest::IncrementalStatus::FullRebuild { .. } => {
                    prepare_directory(&classes_dir)?;
                    compile_pipeline(
                        &main_compilers,
                        &project.source_dirs,
                        &main_compile_classpath,
                        &classes_dir,
                        &project.project_root,
                        jvm_target,
                        None,
                    )?;
                    true
                }
                manifest::IncrementalStatus::Incremental { dirty } => {
                    ensure_directory(&classes_dir)?;
                    let dirty_set: HashSet<PathBuf> = dirty.iter().cloned().collect();
                    compile_pipeline(
                        &main_compilers,
                        &project.source_dirs,
                        &main_compile_classpath,
                        &classes_dir,
                        &project.project_root,
                        jvm_target,
                        Some(&dirty_set),
                    )?;
                    true
                }
                manifest::IncrementalStatus::UpToDate => {
                    ensure_directory(&classes_dir)?;
                    false
                }
            };

            copy_resources(&project.resource_dir, &classes_dir)?;

            if main_compiled {
                try_save_manifest(
                    &main_manifest_path,
                    toolchain_hash.clone(),
                    main_classpath_hash,
                    &all_main_sources,
                );
            }
        } else {
            ensure_directory(&classes_dir)?;
        }

        let test_java_sources = jot_common::collect_files_by_ext(&project.test_source_dirs, "java");
        let test_kotlin_sources = jot_common::collect_files_by_ext(&project.test_source_dirs, "kt");

        if test_java_sources.is_empty() && test_kotlin_sources.is_empty() {
            return Ok(TestOutput {
                project,
                tests_found: false,
            });
        }

        let test_classes_dir = target_dir.join("test-classes");
        let test_compile_classpath = ClasspathAssembler::new()
            .with_paths([classes_dir.clone()])
            .with_artifacts(&compile_dependencies)
            .with_paths(path_dependency_jars.iter().cloned())
            .with_artifacts(&test_dependencies)
            .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
            .build();

        let test_classpath_hash = manifest::compute_classpath_hash(&test_compile_classpath);
        let test_manifest_path = target_dir.join(manifest::TEST_MANIFEST_FILENAME);
        let test_existing = manifest::BuildManifest::load(&test_manifest_path);

        let all_test_sources: Vec<PathBuf> = {
            let mut v = test_java_sources;
            v.extend(test_kotlin_sources);
            v
        };
        let test_status = manifest::classify_sources(
            test_existing.as_ref(),
            &toolchain_hash,
            &test_classpath_hash,
            &all_test_sources,
        )?;

        // Test sources: no annotation processing
        let test_compilers = build_compiler_chain(
            installed_kotlin.as_ref(),
            &installed_jdk,
            Some(project.test_source_dirs.as_slice()),
            None,
        );

        let test_compiled = match &test_status {
            manifest::IncrementalStatus::FullRebuild { .. } => {
                prepare_directory(&test_classes_dir)?;
                compile_pipeline(
                    &test_compilers,
                    &project.test_source_dirs,
                    &test_compile_classpath,
                    &test_classes_dir,
                    &project.project_root,
                    jvm_target,
                    None,
                )?;
                true
            }
            manifest::IncrementalStatus::Incremental { dirty } => {
                ensure_directory(&test_classes_dir)?;
                let dirty_set: HashSet<PathBuf> = dirty.iter().cloned().collect();
                compile_pipeline(
                    &test_compilers,
                    &project.test_source_dirs,
                    &test_compile_classpath,
                    &test_classes_dir,
                    &project.project_root,
                    jvm_target,
                    Some(&dirty_set),
                )?;
                true
            }
            manifest::IncrementalStatus::UpToDate => {
                ensure_directory(&test_classes_dir)?;
                false
            }
        };

        if test_compiled {
            try_save_manifest(
                &test_manifest_path,
                toolchain_hash,
                test_classpath_hash,
                &all_test_sources,
            );
        }

        let console_jar = test_dependencies
            .iter()
            .find(|item| {
                item.coordinate.group == "org.junit.platform"
                    && item.coordinate.artifact == "junit-platform-console-standalone"
            })
            .map(|item| item.path.clone())
            .ok_or(BuildError::MissingJUnitConsole)?;

        let runtime_classpath = ClasspathAssembler::new()
            .with_paths([classes_dir, test_classes_dir])
            .with_artifacts(&compile_dependencies)
            .with_paths(path_dependency_jars)
            .with_artifacts(&test_dependencies)
            .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
            .build();
        let classpath = std::env::join_paths(&runtime_classpath)?;

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

    pub fn bench(
        &self,
        start: &Path,
        filter: Option<&str>,
        forks: u32,
        warmup: Option<u32>,
        iterations: Option<u32>,
    ) -> Result<BenchOutput, BuildError> {
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

        let jmh_version = project
            .bench
            .jmh_version
            .as_deref()
            .unwrap_or(DEFAULT_JMH_VERSION);
        let jmh_core_coord = format!("org.openjdk.jmh:jmh-core:{jmh_version}");
        let jmh_annprocess_coord =
            format!("org.openjdk.jmh:jmh-generator-annprocess:{jmh_version}");

        let compile_dependencies = self
            .resolver
            .resolve_artifacts(&project.dependencies, DEFAULT_RESOLVE_DEPTH)?;

        let mut jmh_dep_inputs = vec![jmh_core_coord, jmh_annprocess_coord];
        jmh_dep_inputs.extend(project.bench.deps.iter().cloned());
        let jmh_dependencies = self
            .resolver
            .resolve_artifacts(&jmh_dep_inputs, DEFAULT_RESOLVE_DEPTH)?;

        let target_dir = project.project_root.join("target");
        let classes_dir = target_dir.join("classes");

        let main_java_sources = jot_common::collect_files_by_ext(&project.source_dirs, "java");
        let main_kotlin_sources = jot_common::collect_files_by_ext(&project.source_dirs, "kt");

        let jvm_target = project
            .toolchain
            .as_ref()
            .map(|value| value.version.as_str());

        let kotlin_version = project.kotlin_toolchain.as_ref().map(|tc| tc.version.as_str());
        let toolchain_hash =
            manifest::compute_toolchain_hash(&toolchain_request.version, kotlin_version);

        if !main_java_sources.is_empty() || !main_kotlin_sources.is_empty() {
            let main_compile_classpath = ClasspathAssembler::new()
                .with_artifacts(&compile_dependencies)
                .with_paths(path_dependency_jars.iter().cloned())
                .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
                .build();

            let annotation_processing =
                resolve_annotation_processing(&project, &self.resolver, &target_dir)?;
            let mut hash_paths = main_compile_classpath.clone();
            if let Some(ap) = &annotation_processing {
                hash_paths.extend_from_slice(&ap.processor_paths);
            }
            let main_classpath_hash = manifest::compute_classpath_hash(&hash_paths);
            let main_manifest_path = target_dir.join(manifest::MANIFEST_FILENAME);
            let main_existing = manifest::BuildManifest::load(&main_manifest_path);

            let all_main_sources: Vec<PathBuf> = {
                let mut v = main_java_sources;
                v.extend(main_kotlin_sources);
                v
            };
            let main_status = manifest::classify_sources(
                main_existing.as_ref(),
                &toolchain_hash,
                &main_classpath_hash,
                &all_main_sources,
            )?;

            let main_compilers = build_compiler_chain(
                installed_kotlin.as_ref(),
                &installed_jdk,
                Some(project.source_dirs.as_slice()),
                annotation_processing,
            );

            let main_compiled = match &main_status {
                manifest::IncrementalStatus::FullRebuild { .. } => {
                    prepare_directory(&classes_dir)?;
                    compile_pipeline(
                        &main_compilers,
                        &project.source_dirs,
                        &main_compile_classpath,
                        &classes_dir,
                        &project.project_root,
                        jvm_target,
                        None,
                    )?;
                    true
                }
                manifest::IncrementalStatus::Incremental { dirty } => {
                    ensure_directory(&classes_dir)?;
                    let dirty_set: HashSet<PathBuf> = dirty.iter().cloned().collect();
                    compile_pipeline(
                        &main_compilers,
                        &project.source_dirs,
                        &main_compile_classpath,
                        &classes_dir,
                        &project.project_root,
                        jvm_target,
                        Some(&dirty_set),
                    )?;
                    true
                }
                manifest::IncrementalStatus::UpToDate => {
                    ensure_directory(&classes_dir)?;
                    false
                }
            };

            copy_resources(&project.resource_dir, &classes_dir)?;

            if main_compiled {
                try_save_manifest(
                    &main_manifest_path,
                    toolchain_hash.clone(),
                    main_classpath_hash,
                    &all_main_sources,
                );
            }
        } else {
            ensure_directory(&classes_dir)?;
        }

        let bench_java_sources =
            jot_common::collect_files_by_ext(&project.bench.source_dirs, "java");
        let bench_kotlin_sources =
            jot_common::collect_files_by_ext(&project.bench.source_dirs, "kt");

        if bench_java_sources.is_empty() && bench_kotlin_sources.is_empty() {
            return Ok(BenchOutput {
                project,
                benchmarks_found: false,
            });
        }

        let bench_classes_dir = target_dir.join("bench-classes");

        let bench_compile_classpath = ClasspathAssembler::new()
            .with_paths([classes_dir.clone()])
            .with_artifacts(&compile_dependencies)
            .with_paths(path_dependency_jars.iter().cloned())
            .with_artifacts(&jmh_dependencies)
            .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
            .build();

        // Verify the annotation processor is present in the resolved deps
        jmh_dependencies
            .iter()
            .find(|a| a.coordinate.artifact == "jmh-generator-annprocess")
            .ok_or(BuildError::MissingJmhAnnotationProcessor)?;

        // All JMH deps go on processor path so jmh-generator-annprocess can load jmh-generator-core
        let jmh_processor_paths: Vec<_> = jmh_dependencies.iter().map(|a| a.path.clone()).collect();

        let generated_bench_sources_dir = target_dir.join("generated-bench-sources");

        // Compute bench classpath hash including processor paths.
        let mut bench_hash_paths = bench_compile_classpath.clone();
        bench_hash_paths.extend_from_slice(&jmh_processor_paths);
        let bench_classpath_hash = manifest::compute_classpath_hash(&bench_hash_paths);
        let bench_manifest_path = target_dir.join(manifest::BENCH_MANIFEST_FILENAME);
        let bench_existing = manifest::BuildManifest::load(&bench_manifest_path);

        let all_bench_sources: Vec<PathBuf> = {
            let mut v = bench_java_sources;
            v.extend(bench_kotlin_sources);
            v
        };
        let bench_status = manifest::classify_sources(
            bench_existing.as_ref(),
            &toolchain_hash,
            &bench_classpath_hash,
            &all_bench_sources,
        )?;

        let bench_ap = Some(AnnotationProcessingConfig {
            processor_paths: jmh_processor_paths,
            options: std::collections::BTreeMap::new(),
            generated_sources_dir: generated_bench_sources_dir.clone(),
        });
        let bench_compilers = build_compiler_chain(
            installed_kotlin.as_ref(),
            &installed_jdk,
            Some(project.bench.source_dirs.as_slice()),
            bench_ap,
        );

        match &bench_status {
            manifest::IncrementalStatus::FullRebuild { .. } => {
                prepare_directory(&bench_classes_dir)?;
                prepare_directory(&generated_bench_sources_dir)?;
                compile_pipeline(
                    &bench_compilers,
                    &project.bench.source_dirs,
                    &bench_compile_classpath,
                    &bench_classes_dir,
                    &project.project_root,
                    jvm_target,
                    None,
                )?;
                try_save_manifest(
                    &bench_manifest_path,
                    toolchain_hash,
                    bench_classpath_hash,
                    &all_bench_sources,
                );
            }
            manifest::IncrementalStatus::Incremental { dirty } => {
                ensure_directory(&bench_classes_dir)?;
                ensure_directory(&generated_bench_sources_dir)?;
                let dirty_set: HashSet<PathBuf> = dirty.iter().cloned().collect();
                compile_pipeline(
                    &bench_compilers,
                    &project.bench.source_dirs,
                    &bench_compile_classpath,
                    &bench_classes_dir,
                    &project.project_root,
                    jvm_target,
                    Some(&dirty_set),
                )?;
                try_save_manifest(
                    &bench_manifest_path,
                    toolchain_hash,
                    bench_classpath_hash,
                    &all_bench_sources,
                );
            }
            manifest::IncrementalStatus::UpToDate => {
                ensure_directory(&bench_classes_dir)?;
                ensure_directory(&generated_bench_sources_dir)?;
            }
        }

        let runtime_classpath = ClasspathAssembler::new()
            .with_paths([classes_dir, bench_classes_dir])
            .with_artifacts(&compile_dependencies)
            .with_paths(path_dependency_jars)
            .with_artifacts(&jmh_dependencies)
            .with_optional_unique_path(kotlin_stdlib_jar(installed_kotlin.as_ref()))
            .build();
        let classpath = std::env::join_paths(&runtime_classpath)?;

        let mut cmd = Command::new(installed_jdk.java_binary());
        cmd.current_dir(&project.project_root)
            .arg("-cp")
            .arg(classpath)
            .arg("org.openjdk.jmh.Main");

        if let Some(filter) = filter {
            cmd.arg(filter);
        }
        cmd.args(["-f", &forks.to_string()]);
        if let Some(w) = warmup {
            cmd.args(["-wi", &w.to_string()]);
        }
        if let Some(i) = iterations {
            cmd.args(["-i", &i.to_string()]);
        }

        let status = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if !status.success() {
            return Err(BuildError::ProcessExit {
                tool: "jmh",
                code: status.code(),
            });
        }

        Ok(BenchOutput {
            project,
            benchmarks_found: true,
        })
    }

    pub fn doc(&self, start: &Path) -> Result<DocOutput, BuildError> {
        let project = load_project_build_config(start)?;
        let toolchain_request = project
            .toolchain
            .clone()
            .ok_or_else(|| BuildError::MissingJavaToolchain(project.config_path.clone()))?;
        let installed_jdk = self.toolchains.ensure_installed(&toolchain_request)?;

        let dependencies = self
            .resolver
            .resolve_artifacts(&project.dependencies, DEFAULT_RESOLVE_DEPTH)?;

        let docs_dir = project.project_root.join("target").join("docs");
        prepare_directory(&docs_dir)?;

        let classpath = ClasspathAssembler::new()
            .with_artifacts(&dependencies)
            .build();

        doc::run_dokka(
            &self.resolver,
            &installed_jdk,
            &project,
            &classpath,
            &docs_dir,
        )?;

        Ok(DocOutput { project, docs_dir })
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

#[derive(Debug)]
pub struct BenchOutput {
    pub project: ProjectBuildConfig,
    pub benchmarks_found: bool,
}

#[derive(Debug)]
pub struct DocOutput {
    pub project: ProjectBuildConfig,
    pub docs_dir: PathBuf,
}

#[derive(Default)]
struct ClasspathAssembler {
    entries: Vec<PathBuf>,
}

impl ClasspathAssembler {
    fn new() -> Self {
        Self::default()
    }

    fn with_artifacts(mut self, artifacts: &[ResolvedArtifact]) -> Self {
        self.entries
            .extend(artifacts.iter().map(|artifact| artifact.path.clone()));
        self
    }

    fn with_paths<I>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = PathBuf>,
    {
        self.entries.extend(paths);
        self
    }

    fn with_optional_unique_path(mut self, path: Option<PathBuf>) -> Self {
        if let Some(path) = path
            && !self.entries.contains(&path)
        {
            self.entries.push(path);
        }
        self
    }

    fn build(self) -> Vec<PathBuf> {
        self.entries
    }
}

fn kotlin_stdlib_jar(installed_kotlin: Option<&InstalledKotlin>) -> Option<PathBuf> {
    installed_kotlin
        .map(InstalledKotlin::kotlin_stdlib_jar)
        .filter(|path| path.is_file())
}

fn prepare_directory(path: &Path) -> Result<(), BuildError> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

/// Create the directory if it does not already exist, without removing its contents.
/// Used for incremental builds where existing class files should be preserved.
fn ensure_directory(path: &Path) -> Result<(), BuildError> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Build and persist an updated build manifest. Emits a warning to stderr if
/// fingerprinting or writing the manifest fails, but never returns an error —
/// a missing or corrupt manifest simply forces a full rebuild on the next run.
fn try_save_manifest(
    manifest_path: &Path,
    toolchain_hash: String,
    classpath_hash: String,
    sources: &[PathBuf],
) {
    match manifest::build_updated_manifest(toolchain_hash, classpath_hash, sources) {
        Ok(m) => {
            if let Err(e) = m.save(manifest_path) {
                eprintln!(
                    "jot: warning: could not save build manifest {}: {e}",
                    manifest_path.display()
                );
            }
        }
        Err(e) => eprintln!("jot: warning: could not fingerprint sources for build manifest: {e}"),
    }
}

#[cfg(test)]
mod tests {
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

        let sources =
            jot_common::collect_files_by_ext(&[temp.path().join("src/main/java")], "java");
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

    // ── package.rs helper tests ────────────────────────────────────────────

    #[test]
    fn is_safe_zip_path_rejects_traversal() {
        use super::package::{is_safe_zip_path, should_skip_jar_entry};
        assert!(is_safe_zip_path("org/example/Demo.class"));
        assert!(!is_safe_zip_path("/etc/passwd"));
        assert!(!is_safe_zip_path("foo/../bar"));
        assert!(is_safe_zip_path("META-INF/MANIFEST.MF"));

        assert!(should_skip_jar_entry("META-INF/MANIFEST.MF"));
        assert!(should_skip_jar_entry("META-INF/DEMO.SF"));
        assert!(should_skip_jar_entry("META-INF/DEMO.RSA"));
        assert!(should_skip_jar_entry("META-INF/DEMO.DSA"));
        assert!(!should_skip_jar_entry("META-INF/services/demo.Service"));
        assert!(!should_skip_jar_entry("org/example/Demo.class"));
    }

    #[test]
    fn is_service_file_matches_meta_inf_services() {
        use super::package::is_service_file;
        assert!(is_service_file("META-INF/services/demo.Service"));
        assert!(!is_service_file("META-INF/MANIFEST.MF"));
        assert!(!is_service_file("org/example/Demo.class"));
    }

    #[test]
    fn merge_service_contents_skips_comments_and_blanks() {
        let mut services = BTreeMap::new();
        merge_service_contents(
            &mut services,
            "META-INF/services/demo.Service",
            b"# comment\n\na.Provider\n  \nb.Provider\n",
        );
        assert_eq!(
            services.get("META-INF/services/demo.Service").cloned(),
            Some(vec!["a.Provider".to_owned(), "b.Provider".to_owned()])
        );
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
