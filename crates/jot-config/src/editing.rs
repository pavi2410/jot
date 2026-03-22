use std::fs;
use std::path::Path;

use jot_toolchain::JavaToolchainRequest;
use toml_edit::{DocumentMut, Item, Table, Value, value};

use crate::errors::ConfigError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencySpec {
    Coords(String),
    Catalog(String),
}

pub fn pin_java_toolchain(path: &Path, request: &JavaToolchainRequest) -> Result<(), ConfigError> {
    let content = fs::read_to_string(path)?;
    let mut document = content.parse::<DocumentMut>()?;

    let toolchains = document
        .entry("toolchains")
        .or_insert(Item::Table(Table::new()));
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

pub fn add_dependency(
    path: &Path,
    name: &str,
    spec: DependencySpec,
    test: bool,
) -> Result<(), ConfigError> {
    let content = fs::read_to_string(path)?;
    let mut document = content.parse::<DocumentMut>()?;
    let section_name = if test {
        "test-dependencies"
    } else {
        "dependencies"
    };

    let section = document
        .entry(section_name)
        .or_insert(Item::Table(Table::new()));
    if !section.is_table() {
        *section = Item::Table(Table::new());
    }

    section[name] = match spec {
        DependencySpec::Coords(coords) => value(coords),
        DependencySpec::Catalog(alias) => {
            let mut table = toml_edit::InlineTable::new();
            table.insert("catalog", Value::from(alias));
            value(table)
        }
    };

    fs::write(path, document.to_string())?;
    Ok(())
}

pub fn remove_dependency(path: &Path, name: &str, test: bool) -> Result<bool, ConfigError> {
    let content = fs::read_to_string(path)?;
    let mut document = content.parse::<DocumentMut>()?;
    let section_name = if test {
        "test-dependencies"
    } else {
        "dependencies"
    };

    let mut removed = false;
    if let Some(section) = document.get_mut(section_name)
        && let Some(table) = section.as_table_mut()
    {
        removed = table.remove(name).is_some();
    }

    if removed {
        fs::write(path, document.to_string())?;
    }

    Ok(removed)
}
