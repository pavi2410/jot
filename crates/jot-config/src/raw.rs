use jot_toolchain::JdkVendor;
use serde::Deserialize;

use crate::models::JavaFormatStyle;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawConfig {
    pub(crate) project: Option<RawProject>,
    pub(crate) workspace: Option<RawWorkspace>,
    pub(crate) dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    #[serde(rename = "test-dependencies")]
    pub(crate) test_dependencies: Option<std::collections::BTreeMap<String, RawDependencySpec>>,
    pub(crate) toolchains: Option<RawToolchains>,
    pub(crate) format: Option<RawFormat>,
    pub(crate) lint: Option<RawLint>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawProject {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(rename = "main-class")]
    pub(crate) main_class: Option<String>,
    #[serde(rename = "source-dirs")]
    pub(crate) source_dirs: Option<Vec<String>>,
    #[serde(rename = "test-source-dirs")]
    pub(crate) test_source_dirs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawWorkspace {
    pub(crate) members: Vec<String>,
    pub(crate) group: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawCatalog {
    pub(crate) versions: Option<std::collections::BTreeMap<String, String>>,
    pub(crate) libraries: Option<std::collections::BTreeMap<String, RawCatalogLibrary>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawCatalogLibrary {
    pub(crate) module: String,
    pub(crate) version: Option<RawCatalogVersion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawCatalogVersion {
    Literal(String),
    Detailed { r#ref: String },
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawToolchains {
    pub(crate) java: Option<RawJavaToolchain>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawJavaToolchain {
    Version(String),
    Detailed {
        version: String,
        vendor: Option<JdkVendor>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawDependencySpec {
    Coords(String),
    Detailed {
        coords: Option<String>,
        path: Option<String>,
        catalog: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawFormat {
    #[serde(rename = "java-style")]
    pub(crate) java_style: Option<JavaFormatStyle>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawLint {
    #[serde(rename = "pmd-ruleset")]
    pub(crate) pmd_ruleset: Option<String>,
}
