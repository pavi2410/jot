pub mod coordinate;
pub mod errors;
mod models;
mod resolver;
mod versions;

pub use coordinate::{MavenCoordinate, ResolvedCoordinate};
pub use errors::ResolverError;
pub use models::{
    MavenDependencies, MavenDependency, MavenDeveloper, MavenDevelopers, MavenExclusion,
    MavenExclusions, MavenLicense, MavenLicenses, MavenMetadata, MavenParent, MavenProject,
    MavenScope, MavenScm, MavenVersioning, MavenVersions,
};
pub use resolver::{
    LockedPackage, Lockfile, MavenResolver, ResolvedArtifact, ResolvedDependency, TreeEntry,
};

#[cfg(test)]
mod tests {
    use crate::coordinate::{MavenCoordinate, ResolvedCoordinate};
    use crate::models::{
        MavenDependencies, MavenDependency, MavenDependencyManagement, MavenDistributionManagement,
        MavenExclusion, MavenExclusions, MavenParent, MavenProject, MavenRelocation, MavenScope,
        MavenVersioning, MavenVersions,
    };
    use crate::resolver::{
        LockedPackage, Lockfile, ResolvedDependency, dependency_exclusions,
        include_classpath_scope, interpolate_value, is_cache_usable, normalize_checksum_response,
        relocation_target,
    };
    use crate::versions::{
        is_property_version_expression, is_stable_maven_version, needs_dynamic_version_resolution,
        parse_version_spec, resolve_best_version, resolve_version_from_metadata,
        version_matches_any_range,
    };
    use quick_xml::de::from_str;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn parses_coordinates_with_optional_version() {
        let simple = MavenCoordinate::parse("org.junit.jupiter:junit-jupiter").expect("parse");
        assert_eq!(simple.group, "org.junit.jupiter");
        assert_eq!(simple.artifact, "junit-jupiter");
        assert_eq!(simple.version, None);
        assert_eq!(simple.classifier, None);

        let pinned =
            MavenCoordinate::parse("org.junit.jupiter:junit-jupiter:5.11.0").expect("parse");
        assert_eq!(pinned.version.as_deref(), Some("5.11.0"));

        let classified = MavenCoordinate::parse("org.junit.jupiter:junit-jupiter:5.11.0:sources")
            .expect("parse classified coordinate");
        assert_eq!(classified.classifier.as_deref(), Some("sources"));
        assert_eq!(
            classified.to_string(),
            "org.junit.jupiter:junit-jupiter:5.11.0:sources"
        );
    }

    #[test]
    fn best_version_prefers_release_then_latest_then_last_listed() {
        let with_release = MavenVersioning {
            latest: Some("2.0.0".into()),
            release: Some("1.9.0".into()),
            versions: Some(MavenVersions {
                version: vec!["1.0.0".into(), "1.9.0".into()],
            }),
        };
        assert_eq!(
            resolve_best_version(&with_release).as_deref(),
            Some("1.9.0")
        );

        let with_latest = MavenVersioning {
            latest: Some("2.0.0".into()),
            release: None,
            versions: None,
        };
        assert_eq!(resolve_best_version(&with_latest).as_deref(), Some("2.0.0"));

        let with_versions = MavenVersioning {
            latest: None,
            release: None,
            versions: Some(MavenVersions {
                version: vec!["1.0.0".into(), "1.1.0".into(), "1.2.0".into()],
            }),
        };
        assert_eq!(
            resolve_best_version(&with_versions).as_deref(),
            Some("1.2.0")
        );

        let prefers_stable = MavenVersioning {
            latest: Some("2.0.0-RC1".into()),
            release: Some("2.0.0-M1".into()),
            versions: Some(MavenVersions {
                version: vec!["1.9.9".into(), "2.0.0-M1".into(), "2.0.0-RC1".into()],
            }),
        };
        assert_eq!(
            resolve_best_version(&prefers_stable).as_deref(),
            Some("1.9.9")
        );
    }

    #[test]
    fn include_classpath_scope_excludes_test_provided_and_import() {
        assert!(include_classpath_scope(None));
        assert!(include_classpath_scope(Some(MavenScope::Compile)));
        assert!(include_classpath_scope(Some(MavenScope::Runtime)));
        assert!(!include_classpath_scope(Some(MavenScope::Test)));
        assert!(!include_classpath_scope(Some(MavenScope::Provided)));
        assert!(!include_classpath_scope(Some(MavenScope::Import)));
    }

    #[test]
    fn resolves_dynamic_versions_from_metadata() {
        let versioning = MavenVersioning {
            latest: Some("2.1.0".into()),
            release: Some("2.0.0".into()),
            versions: Some(MavenVersions {
                version: vec![
                    "1.0.0".into(),
                    "1.5.0".into(),
                    "1.9.9".into(),
                    "2.0.0-RC1".into(),
                    "2.0.0".into(),
                    "2.1.0".into(),
                ],
            }),
        };

        assert_eq!(
            resolve_version_from_metadata(&versioning, "LATEST").as_deref(),
            Some("2.1.0")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "RELEASE").as_deref(),
            Some("2.0.0")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "[1.5,2.0)").as_deref(),
            Some("1.9.9")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "(,1.0.0]").as_deref(),
            Some("1.0.0")
        );
        assert_eq!(
            resolve_version_from_metadata(&versioning, "[2.0.0,)").as_deref(),
            Some("2.1.0")
        );
    }

    #[test]
    fn parses_and_matches_union_ranges() {
        let intervals = parse_version_spec("(,1.0],[1.2,)").expect("parse ranges");
        assert!(version_matches_any_range("0.9", &intervals));
        assert!(!version_matches_any_range("1.1", &intervals));
        assert!(version_matches_any_range("1.2", &intervals));
        assert!(needs_dynamic_version_resolution("[1.0,2.0)"));
        assert!(!needs_dynamic_version_resolution("1.2.3"));
    }

    #[test]
    fn stable_version_filter_accepts_and_rejects_expected_formats() {
        assert!(is_stable_maven_version("1.2.3"));
        assert!(is_stable_maven_version("1.2.3.Final"));
        assert!(!is_stable_maven_version("1.2.3-SNAPSHOT"));
        assert!(!is_stable_maven_version("2.0.0-M1"));
        assert!(!is_stable_maven_version("2.0.0-RC1"));
    }

    #[test]
    fn parses_maven_dependencies_block_from_pom_xml() {
        let xml = r#"
                        <project>
                            <dependencies>
                                <dependency>
                                    <groupId>org.junit.jupiter</groupId>
                                    <artifactId>junit-jupiter-api</artifactId>
                                    <version>5.11.0</version>
                                    <scope>test</scope>
                                    <optional>false</optional>
                                </dependency>
                            </dependencies>
                        </project>
                "#;

        let project: MavenProject = from_str(xml).expect("parse pom");
        let dependencies = project.dependencies.expect("dependencies").dependency;
        assert_eq!(dependencies.len(), 1);
        let first: &MavenDependency = &dependencies[0];
        assert_eq!(first.group_id.as_deref(), Some("org.junit.jupiter"));
        assert_eq!(first.artifact_id.as_deref(), Some("junit-jupiter-api"));
        assert_eq!(first.version.as_deref(), Some("5.11.0"));
        assert_eq!(first.scope, Some(MavenScope::Test));
        assert_eq!(first.optional, Some(false));
    }

    #[test]
    fn property_version_expression_detection_matches_expected_cases() {
        assert!(is_property_version_expression("${junit.version}"));
        assert!(!is_property_version_expression("[1.0,2.0)"));
        assert!(!is_property_version_expression("(,1.4.0]"));
        assert!(!is_property_version_expression("1.2.3"));
    }

    #[test]
    fn dependency_to_coordinate_requires_literal_version() {
        let literal = ResolvedDependency {
            group: "org.example".into(),
            artifact: "demo".into(),
            version: Some("1.0.0".into()),
            classifier: Some("tests".into()),
            scope: None,
            optional: false,
            exclusions: BTreeSet::new(),
        };
        assert_eq!(literal.classifier.as_deref(), Some("tests"));

        let managed = ResolvedDependency {
            group: "org.example".into(),
            artifact: "demo".into(),
            version: Some("${demo.version}".into()),
            classifier: None,
            scope: None,
            optional: false,
            exclusions: BTreeSet::new(),
        };
        assert!(is_property_version_expression(
            managed.version.as_deref().expect("managed version")
        ));
    }

    #[test]
    fn cache_usability_respects_file_age_when_ttl_is_present() {
        let temp = tempdir().expect("tempdir");
        let file_path = temp.path().join("metadata.xml");
        fs::write(&file_path, "<metadata />").expect("write metadata");

        assert!(is_cache_usable(&file_path, Some(Duration::from_secs(60))).expect("fresh cache"));
        assert!(is_cache_usable(&file_path, None).expect("ttl-free cache"));
    }

    #[test]
    fn lockfile_packages_are_deterministic_and_deduplicated() {
        let lockfile = Lockfile {
            version: 1,
            roots: vec![ResolvedCoordinate {
                group: "org.example".into(),
                artifact: "demo".into(),
                version: "1.0.0".into(),
                classifier: None,
            }],
            package: vec![
                LockedPackage {
                    group: "b.group".into(),
                    artifact: "beta".into(),
                    version: "1.0.0".into(),
                    classifier: None,
                    sha256: "abc123".into(),
                },
                LockedPackage {
                    group: "a.group".into(),
                    artifact: "alpha".into(),
                    version: "2.0.0".into(),
                    classifier: Some("sources".into()),
                    sha256: "def456".into(),
                },
            ],
        };

        assert_eq!(lockfile.package[0].group, "b.group");
        assert_eq!(lockfile.package[1].group, "a.group");
    }

    #[test]
    fn interpolation_replaces_known_properties_and_keeps_unknown() {
        let mut properties = BTreeMap::new();
        properties.insert("junit.version".to_owned(), "5.11.0".to_owned());

        assert_eq!(
            interpolate_value("org.junit:junit-bom:${junit.version}", &properties),
            "org.junit:junit-bom:5.11.0"
        );
        assert_eq!(
            interpolate_value("${missing.value}", &properties),
            "${missing.value}"
        );
    }

    #[test]
    fn managed_versions_fill_dependency_versions() {
        let project = MavenProject {
            group_id: Some("org.example".to_owned()),
            artifact_id: Some("demo".to_owned()),
            version: Some("1.0.0".to_owned()),
            parent: Some(MavenParent {
                group_id: Some("org.example".to_owned()),
                artifact_id: Some("parent".to_owned()),
                version: Some("1.0.0".to_owned()),
            }),
            dependency_management: Some(MavenDependencyManagement {
                dependencies: MavenDependencies {
                    dependency: vec![MavenDependency {
                        group_id: Some("org.slf4j".to_owned()),
                        artifact_id: Some("slf4j-api".to_owned()),
                        version: Some("2.0.16".to_owned()),
                        packaging: None,
                        classifier: None,
                        scope: None,
                        optional: None,
                        exclusions: None,
                    }],
                },
            }),
            dependencies: Some(MavenDependencies {
                dependency: vec![MavenDependency {
                    group_id: Some("org.slf4j".to_owned()),
                    artifact_id: Some("slf4j-api".to_owned()),
                    version: None,
                    packaging: None,
                    classifier: Some("tests".to_owned()),
                    scope: None,
                    optional: None,
                    exclusions: None,
                }],
            }),
            ..Default::default()
        };

        assert_eq!(
            project
                .dependency_management
                .as_ref()
                .expect("management")
                .dependencies
                .dependency[0]
                .version
                .as_deref(),
            Some("2.0.16")
        );
    }

    #[test]
    fn parses_dependency_exclusions_and_interpolates_values() {
        let dependency = MavenDependency {
            group_id: Some("org.example".to_owned()),
            artifact_id: Some("consumer".to_owned()),
            version: Some("1.0.0".to_owned()),
            packaging: None,
            classifier: None,
            scope: None,
            optional: None,
            exclusions: Some(MavenExclusions {
                exclusion: vec![MavenExclusion {
                    group_id: Some("${excluded.group}".to_owned()),
                    artifact_id: Some("${excluded.artifact}".to_owned()),
                }],
            }),
        };

        let mut properties = BTreeMap::new();
        properties.insert("excluded.group".to_owned(), "org.slf4j".to_owned());
        properties.insert("excluded.artifact".to_owned(), "slf4j-api".to_owned());

        let exclusions = dependency_exclusions(&dependency, &properties);
        assert!(exclusions.contains(&("org.slf4j".to_owned(), "slf4j-api".to_owned())));
    }

    #[test]
    fn normalizes_checksum_sidecar_contents() {
        assert_eq!(
            normalize_checksum_response("ABCDEF0123456789  demo.jar\n"),
            Some("abcdef0123456789".to_owned())
        );
        assert_eq!(normalize_checksum_response(""), None);
    }

    #[test]
    fn sha256_file_hashes_contents() {
        let temp = tempdir().expect("tempdir");
        let file_path = temp.path().join("demo.jar");
        fs::write(&file_path, b"hello world").expect("write artifact");

        let checksum = jot_common::sha256_file(&file_path).expect("sha256");
        assert_eq!(
            checksum,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn relocation_target_uses_relocation_fields_with_coordinate_fallbacks() {
        let project = MavenProject {
            group_id: Some("legacy.group".to_owned()),
            artifact_id: Some("legacy-artifact".to_owned()),
            version: Some("1.0.0".to_owned()),
            distribution_management: Some(MavenDistributionManagement {
                relocation: Some(MavenRelocation {
                    group_id: Some("modern.group".to_owned()),
                    artifact_id: Some("modern-artifact".to_owned()),
                    version: Some("2.0.0".to_owned()),
                    classifier: Some("sources".to_owned()),
                }),
            }),
            ..Default::default()
        };

        let resolved = relocation_target(
            &project,
            &MavenCoordinate {
                group: "legacy.group".to_owned(),
                artifact: "legacy-artifact".to_owned(),
                version: Some("1.0.0".to_owned()),
                classifier: None,
            },
        )
        .expect("relocation target");

        assert_eq!(
            resolved.to_string(),
            "modern.group:modern-artifact:2.0.0:sources"
        );
    }
}
