use std::fs;
use std::path::{Path, PathBuf};

use jot_toolchain::{JavaToolchainRequest, JdkVendor};
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, Value, value};

#[derive(Debug, Deserialize)]
struct RawConfig {
    toolchains: Option<RawToolchains>,
}

#[derive(Debug, Deserialize)]
struct RawToolchains {
    java: Option<RawJavaToolchain>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawJavaToolchain {
    Version(String),
    Detailed {
        version: String,
        vendor: Option<JdkVendor>,
    },
}

pub fn find_jot_toml(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()
            .ok_or_else(|| ConfigError::InvalidStartPath(start.to_path_buf()))?
            .to_path_buf()
    };

    loop {
        let candidate = current.join("jot.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn read_toolchain_request(start: &Path) -> Result<Option<JavaToolchainRequest>, ConfigError> {
    let Some(path) = find_jot_toml(start)? else {
        return Ok(None);
    };

    let content = fs::read_to_string(path)?;
    let config: RawConfig = toml::from_str(&content)?;
    let Some(toolchains) = config.toolchains else {
        return Ok(None);
    };

    Ok(toolchains.java.map(|java| match java {
        RawJavaToolchain::Version(version) => JavaToolchainRequest {
            version,
            vendor: None,
        },
        RawJavaToolchain::Detailed { version, vendor } => JavaToolchainRequest {
            version,
            vendor,
        },
    }))
}

pub fn pin_java_toolchain(path: &Path, request: &JavaToolchainRequest) -> Result<(), ConfigError> {
    let content = fs::read_to_string(path)?;
    let mut document = content.parse::<DocumentMut>()?;

    let toolchains = document.entry("toolchains").or_insert(Item::Table(Table::new()));
    if !toolchains.is_table() {
        *toolchains = Item::Table(Table::new());
    }

    let java_item = match request.vendor {
        Some(vendor) => {
            let mut table = toml_edit::InlineTable::new();
            table.insert("version", Value::from(request.version.as_str()));
            table.insert("vendor", Value::from(vendor.to_string()));
            value(table)
        }
        None => value(request.version.as_str()),
    };
    toolchains["java"] = java_item;

    fs::write(path, document.to_string())?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid start path: {0}")]
    InvalidStartPath(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse jot.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("failed to update jot.toml: {0}")]
    EditToml(#[from] toml_edit::TomlError),
}

#[cfg(test)]
mod tests {
    use super::{JavaToolchainRequest, find_jot_toml, pin_java_toolchain, read_toolchain_request};
    use jot_toolchain::JdkVendor;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_nearest_config_in_parent_directory() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        let nested = root.join("nested").join("deeper");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(root.join("jot.toml"), "[toolchains]\njava = \"21\"\n").expect("write config");

        let path = find_jot_toml(&nested).expect("find config");
        assert_eq!(path, Some(root.join("jot.toml")));
    }

    #[test]
    fn pins_java_toolchain_with_vendor_table() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("jot.toml");
        fs::write(&config_path, "[project]\nname = \"demo\"\n").expect("write config");

        pin_java_toolchain(
            &config_path,
            &JavaToolchainRequest {
                version: "21".into(),
                vendor: Some(JdkVendor::Adoptium),
            },
        )
        .expect("pin toolchain");

        let request = read_toolchain_request(&config_path)
            .expect("read config")
            .expect("toolchain request");
        assert_eq!(request.version, "21");
        assert_eq!(request.vendor, Some(JdkVendor::Adoptium));
    }
}