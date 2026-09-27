use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use super::settings::JsonSettings;
use super::{Host, InstallState};
use crate::paths;

const BINARY_PLACEHOLDER: &str = "__STATUSMAXXX_BINARY__";

struct Shim {
    file: PathBuf,
    source: &'static str,
}

fn shim(host: Host) -> Result<Shim> {
    let home = host.home()?;
    Ok(match host {
        Host::Amp => Shim { file: home.join("plugins").join("statusmaxxx.ts"), source: include_str!("shims/amp.ts") },
        Host::Pi => Shim { file: home.join("extensions").join("statusmaxxx.ts"), source: include_str!("shims/pi.ts") },
        // Not under `plugins/`: OpenCode auto-loads that folder as server plugins.
        Host::Opencode => {
            Shim { file: home.join("tui-plugins").join("statusmaxxx.tsx"), source: include_str!("shims/opencode.tsx") }
        }
        _ => unreachable!("{host} is not a plugin host"),
    })
}

/// OpenCode only loads TUI plugins listed in `tui.json`, relative to that file.
const OPENCODE_SPEC: &str = "./tui-plugins/statusmaxxx.tsx";

pub fn state(host: Host) -> Result<InstallState> {
    Ok(if shim(host)?.file.exists() { InstallState::Installed } else { InstallState::NotInstalled })
}

pub fn install(host: Host) -> Result<Vec<String>> {
    let shim = shim(host)?;
    let binary = serde_json::to_string(&paths::binary()?.to_string_lossy())?;
    paths::write_file(&shim.file, &shim.source.replace(BINARY_PLACEHOLDER, &binary))?;
    let mut report = vec![format!("Wrote <{}>", paths::display(&shim.file))];
    if host == Host::Opencode {
        report.extend(register_opencode_plugin(true)?);
    }
    Ok(report)
}

pub fn uninstall(host: Host) -> Result<Vec<String>> {
    let shim = shim(host)?;
    let mut report = Vec::new();
    if host == Host::Opencode {
        report.extend(register_opencode_plugin(false)?);
    }
    if shim.file.exists() {
        fs::remove_file(&shim.file).with_context(|| format!("Cannot remove <{}>", shim.file.display()))?;
        report.push(format!("Removed <{}>", paths::display(&shim.file)));
    }
    Ok(report)
}

fn register_opencode_plugin(registered: bool) -> Result<Option<String>> {
    let mut settings = JsonSettings::open(Host::Opencode.home()?.join("tui.json"))?;
    let mut plugins: Vec<Value> = match settings.get(&["plugin"]) {
        Some(Value::Array(plugins)) => plugins.clone(),
        _ => Vec::new(),
    };
    let is_ours = |plugin: &Value| match plugin {
        Value::String(spec) => spec == OPENCODE_SPEC,
        Value::Array(entry) => entry.first().and_then(Value::as_str) == Some(OPENCODE_SPEC),
        _ => false,
    };
    let present = plugins.iter().any(is_ours);
    if present == registered {
        return Ok(None);
    }
    if registered {
        plugins.push(Value::String(OPENCODE_SPEC.to_string()));
    } else {
        plugins.retain(|plugin| !is_ours(plugin));
    }
    if plugins.is_empty() {
        settings.remove(&["plugin"]);
    } else {
        settings.set(&["plugin"], Value::Array(plugins))?;
    }
    settings.save()?;
    Ok(Some(format!("Updated <{}>", paths::display(settings.path()))))
}
