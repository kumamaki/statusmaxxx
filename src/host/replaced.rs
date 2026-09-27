//! What an install replaced, so uninstall can put the user's setup back.

use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Host;
use crate::paths;

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    previous: Value,
}

/// Keeps the first replaced value: reinstalling must not record our own entry.
pub fn remember(host: Host, previous: Option<&Value>) -> Result<()> {
    let path = paths::replaced_record(host)?;
    match previous {
        Some(previous) if !path.exists() => {
            paths::write_file(&path, &serde_json::to_string_pretty(&Record { previous: previous.clone() })?)
        }
        _ => Ok(()),
    }
}

/// The value to restore, consuming the record.
pub fn take(host: Host) -> Result<Option<Value>> {
    let path = paths::replaced_record(host)?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
    };
    let record: Record = serde_json::from_str(&contents).with_context(|| format!("<{}> is corrupt", path.display()))?;
    fs::remove_file(&path).with_context(|| format!("Cannot remove <{}>", path.display()))?;
    Ok(Some(record.previous))
}
