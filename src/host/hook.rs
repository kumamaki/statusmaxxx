//! Session-start hooks: at startup, resume, and after compaction, the agent is
//! told which issue this worktree shows, or how to set one.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::Host;
use super::settings::JsonSettings;
use super::wrapper::Wrapper;
use crate::paths;

enum Target {
    /// A hook list inside a settings file the agent shares with other tools.
    List { file: PathBuf, key: &'static [&'static str], shape: EntryShape },
    /// A hooks file of our own, in a folder the agent loads every file from.
    OwnFile(PathBuf),
}

#[derive(Clone, Copy)]
enum EntryShape {
    /// `{"hooks": [{"type": "command", "command": …}]}`
    Claude,
    /// `{"command": …}`
    Cursor,
}

fn target(host: Host) -> Result<Option<Target>> {
    let home = host.home()?;
    let claude_style =
        |file: &str| Target::List { file: home.join(file), key: &["hooks", "SessionStart"], shape: EntryShape::Claude };
    Ok(match host {
        Host::Claude | Host::Qwen | Host::Droid => Some(claude_style("settings.json")),
        Host::Cursor => Some(Target::List {
            file: home.join("hooks.json"),
            key: &["hooks", "sessionStart"],
            shape: EntryShape::Cursor,
        }),
        Host::Copilot => Some(Target::OwnFile(home.join("hooks").join("statusmaxxx.json"))),
        Host::Amp | Host::Pi | Host::Opencode | Host::Codex | Host::Gemini => None,
    })
}

/// The hook's stdout in the shape each agent reads context from.
pub fn output(host: Host, context: &str) -> Value {
    match host {
        Host::Cursor => json!({ "additional_context": context }),
        Host::Copilot => json!({ "additionalContext": context }),
        // Claude Code's shape, which Qwen and Droid adopted.
        _ => json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": context } }),
    }
}

pub fn install(host: Host) -> Result<Vec<String>> {
    let Some(target) = target(host)? else {
        return Ok(vec![]);
    };
    let mut report = vec![Wrapper::SessionStart.write(host)?];
    let command = Wrapper::SessionStart.command(host)?;
    match target {
        Target::List { file, key, shape } => {
            let mut settings = JsonSettings::open(file)?;
            let mut entries = entries(&settings, key, &command);
            entries.push(match shape {
                EntryShape::Claude => json!({ "hooks": [{ "type": "command", "command": command }] }),
                EntryShape::Cursor => json!({ "command": command }),
            });
            if matches!(shape, EntryShape::Cursor) && settings.get(&["version"]).is_none() {
                settings.set(&["version"], json!(1))?;
            }
            settings.set(key, Value::Array(entries))?;
            settings.save()?;
            report.push(format!("Added the session-start hook to <{}>", paths::display(settings.path())));
        }
        Target::OwnFile(file) => {
            let hooks = json!({ "version": 1, "hooks": { "sessionStart": [{ "type": "command", "bash": command, "timeoutSec": 10 }] } });
            paths::write_file(&file, &format!("{}\n", serde_json::to_string_pretty(&hooks)?))?;
            report.push(format!("Wrote <{}>", paths::display(&file)));
        }
    }
    Ok(report)
}

pub fn uninstall(host: Host) -> Result<Vec<String>> {
    let Some(target) = target(host)? else {
        return Ok(vec![]);
    };
    let command = Wrapper::SessionStart.command(host)?;
    let mut report = Vec::new();
    match target {
        Target::List { file, key, shape } => {
            let mut settings = JsonSettings::open(file)?;
            let before = settings.get(key).and_then(Value::as_array).map_or(0, Vec::len);
            let entries = entries(&settings, key, &command);
            if entries.len() != before {
                if entries.is_empty() {
                    settings.remove(key);
                } else {
                    settings.set(key, Value::Array(entries))?;
                }
                report.push(format!("Removed the session-start hook from <{}>", paths::display(settings.path())));
                // Cursor's hooks.json is left holding only its schema version once our hook is gone.
                if matches!(shape, EntryShape::Cursor) && settings.keys().all(|key| key == "version") {
                    settings.delete()?;
                } else {
                    settings.save()?;
                }
            }
        }
        Target::OwnFile(file) => {
            if file.exists() {
                fs::remove_file(&file).with_context(|| format!("Cannot remove <{}>", file.display()))?;
                report.push(format!("Removed <{}>", paths::display(&file)));
            }
        }
    }
    report.extend(Wrapper::SessionStart.remove(host)?);
    Ok(report)
}

/// The hook list without our entry, so installs stay idempotent.
fn entries(settings: &JsonSettings, key: &[&str], command: &str) -> Vec<Value> {
    let is_ours = |entry: &Value| {
        entry["command"] == command
            || entry["hooks"].as_array().is_some_and(|hooks| hooks.iter().any(|hook| hook["command"] == command))
    };
    settings
        .get(key)
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |entries| entries.iter().filter(|entry| !is_ours(entry)).cloned().collect())
}
