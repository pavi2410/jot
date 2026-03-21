use std::fs;
use std::path::Path;

use jot_toolchain::JavaToolchainRequest;
use toml_edit::{DocumentMut, Item, Table, Value, value};

use crate::errors::ConfigError;

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
