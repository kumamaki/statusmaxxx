use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};

use super::settings::JsonSettings;
use super::wrapper::Wrapper;
use super::{Host, InstallState, replaced};
use crate::paths;

/// Where a command host keeps its status line setting.
struct Target {
    file: PathBuf,
    key: &'static [&'static str],
    entry: Value,
    /// Settings that must also hold for the custom line to show.
    extras: &'static [(&'static [&'static str], bool)],
}

fn target(host: Host, command: &str) -> Result<Target> {
    let home = host.home()?;
    Ok(match host {
        Host::Claude => Target {
            file: home.join("settings.json"),
            key: &["statusLine"],
            entry: json!({ "type": "command", "command": command, "padding": 0 }),
            extras: &[],
        },
        Host::Cursor => Target {
            file: home.join("cli-config.json"),
            key: &["statusLine"],
            entry: json!({ "type": "command", "command": command }),
            extras: &[],
        },
        Host::Qwen => Target {
            file: home.join("settings.json"),
            key: &["ui", "statusLine"],
            entry: json!({ "type": "command", "command": command, "respectUserColors": true }),
            extras: &[],
        },
        Host::Droid => Target {
            file: home.join("settings.json"),
            key: &["statusLine"],
            entry: json!({ "type": "command", "command": command, "maxRows": 1 }),
            extras: &[],
        },
        Host::Copilot => Target {
            file: home.join("settings.json"),
            key: &["statusLine"],
            entry: json!({ "type": "command", "command": command }),
            extras: &[(&["footer", "showCustom"], true)],
        },
        _ => unreachable!("{host} is not a command host"),
    })
}

pub fn state(host: Host) -> Result<InstallState> {
    let wrapper = Wrapper::StatusLine.command(host)?;
    let target = target(host, &wrapper)?;
    let settings = JsonSettings::open(target.file)?;
    Ok(match settings.get(target.key) {
        None | Some(Value::Null) => InstallState::NotInstalled,
        Some(entry) if entry["command"] == wrapper.as_str() => InstallState::Installed,
        Some(entry) => InstallState::Occupied(describe(entry)),
    })
}

pub fn install(host: Host) -> Result<Vec<String>> {
    let mut report = vec![Wrapper::StatusLine.write(host)?];
    let command = Wrapper::StatusLine.command(host)?;
    let target = target(host, &command)?;
    let mut settings = JsonSettings::open(target.file)?;
    let previous = settings.get(target.key).filter(|entry| !entry.is_null() && entry["command"] != command.as_str());
    if let Some(previous) = previous {
        report.push(format!("Replaced status line <{}>; uninstall restores it", describe(previous)));
    }
    replaced::remember(host, previous)?;
    settings.set(target.key, target.entry)?;
    for (key, value) in target.extras {
        settings.set(key, Value::Bool(*value))?;
    }
    settings.save()?;
    report.push(format!("Updated <{}>", paths::display(settings.path())));
    Ok(report)
}

pub fn uninstall(host: Host) -> Result<Vec<String>> {
    let mut report = Vec::new();
    let installed = state(host)? == InstallState::Installed;
    // Taken even when not installed: a line the user set since then is theirs to keep.
    let previous = replaced::take(host)?;
    if installed {
        let target = target(host, &Wrapper::StatusLine.command(host)?)?;
        let mut settings = JsonSettings::open(target.file)?;
        settings.remove(target.key);
        for (key, _) in target.extras {
            settings.remove(key);
        }
        if let Some(previous) = previous {
            report.push(format!("Restored status line <{}>", describe(&previous)));
            settings.set(target.key, previous)?;
        }
        settings.save()?;
        report.push(format!("Updated <{}>", paths::display(settings.path())));
    }
    report.extend(Wrapper::StatusLine.remove(host)?);
    Ok(report)
}

fn describe(entry: &Value) -> String {
    entry["command"].as_str().map_or_else(|| entry.to_string(), str::to_string)
}
