use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use toml_edit::{Array, DocumentMut, Item, Table};

use super::settings::{JsonSettings, backup_path};
use super::{Host, InstallState, replaced};
use crate::config::Config;
use crate::paths;
use crate::segment::Segment;

/// The agent's own item that shows the same thing as `segment`, if any. Near
/// misses stay out: Codex's `branch-changes` is lines changed against the base
/// branch, not uncommitted files.
pub fn item(host: Host, segment: Segment) -> Option<&'static str> {
    match (host, segment) {
        (Host::Codex, Segment::Directory) => Some("current-dir"),
        (Host::Codex, Segment::Branch) => Some("git-branch"),
        (Host::Codex, Segment::Model) => Some("model-with-reasoning"),
        (Host::Codex, Segment::Context) => Some("context-used"),
        (Host::Codex, Segment::Cost) => Some("estimated-thread-cost"),
        (Host::Gemini, Segment::Directory) => Some("workspace"),
        (Host::Gemini, Segment::Branch) => Some("git-branch"),
        (Host::Gemini, Segment::Model) => Some("model-name"),
        (Host::Gemini, Segment::Context) => Some("context-used"),
        _ => None,
    }
}

fn items(host: Host, config: &Config) -> Vec<&'static str> {
    config.segments_for(host).iter().filter_map(|segment| item(host, *segment)).collect()
}

pub fn state(host: Host, config: &Config) -> Result<InstallState> {
    Ok(match current(host)? {
        None => InstallState::NotInstalled,
        Some(current) if current == items(host, config) => InstallState::Installed,
        Some(current) => InstallState::Occupied(current.join(", ")),
    })
}

pub fn install(host: Host, config: &Config) -> Result<Vec<String>> {
    let items = items(host, config);
    if items.is_empty() {
        bail!("None of the configured segments has a {} equivalent", host.label());
    }
    let previous = current(host)?.filter(|current| *current != items);
    replaced::remember(host, previous.map(Value::from).as_ref())?;
    write(host, Some(items.iter().map(|item| item.to_string()).collect()))
}

/// Rewrites the agent's items for `new` when it still shows what `old` gave it, so a
/// list statusmaxxx wrote follows the config and one edited elsewhere stays. Returns
/// the new items when it wrote them.
pub fn sync(host: Host, old: &Config, new: &Config) -> Result<Option<Vec<&'static str>>> {
    let (before, after) = (items(host, old), items(host, new));
    let ours = current(host)?.is_some_and(|current| current.iter().eq(before.iter()));
    if before == after || !ours {
        return Ok(None);
    }
    write(host, Some(after.iter().map(|item| item.to_string()).collect()))?;
    Ok(Some(after))
}

/// Puts back the items we replaced, or the agent's defaults when there were none.
pub fn uninstall(host: Host) -> Result<Vec<String>> {
    let previous = replaced::take(host)?;
    let restored = previous.map(serde_json::from_value).transpose().context("Recorded items are corrupt")?;
    if restored.is_none() && current(host)?.is_none() {
        return Ok(vec![]);
    }
    write(host, restored)
}

fn current(host: Host) -> Result<Option<Vec<String>>> {
    Ok(match host {
        Host::Codex => codex_items(&codex_document()?.1),
        Host::Gemini => gemini_items(&JsonSettings::open(gemini_file()?)?),
        _ => unreachable!("{host} has no built-in items"),
    })
}

/// `None` removes the setting.
fn write(host: Host, items: Option<Vec<String>>) -> Result<Vec<String>> {
    match host {
        Host::Codex => {
            let (path, mut document) = codex_document()?;
            let tui = document.entry("tui").or_insert_with(|| Item::Table(Table::new()));
            let tui = tui.as_table_like_mut().context("`tui` in the Codex config is not a table")?;
            match items {
                Some(items) => {
                    tui.insert("status_line", toml_edit::value(items.into_iter().collect::<Array>()));
                }
                None => {
                    tui.remove("status_line");
                }
            }
            save_toml(&path, &document)
        }
        Host::Gemini => {
            let mut settings = JsonSettings::open(gemini_file()?)?;
            match items {
                Some(items) => settings.set(&["ui", "footer", "items"], Value::from(items))?,
                None => {
                    settings.remove(&["ui", "footer", "items"]);
                }
            }
            settings.save()?;
            Ok(vec![format!("Updated <{}>", paths::display(settings.path()))])
        }
        _ => unreachable!("{host} has no built-in items"),
    }
}

fn codex_document() -> Result<(PathBuf, DocumentMut)> {
    let path = Host::Codex.home()?.join("config.toml");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
    };
    let document = contents.parse().with_context(|| format!("<{}> is not valid TOML", path.display()))?;
    Ok((path, document))
}

fn codex_items(document: &DocumentMut) -> Option<Vec<String>> {
    let array = document.get("tui")?.get("status_line")?.as_array()?;
    Some(array.iter().filter_map(|value| value.as_str().map(str::to_string)).collect())
}

fn save_toml(path: &PathBuf, document: &DocumentMut) -> Result<Vec<String>> {
    if path.exists() {
        fs::copy(path, backup_path(path)).with_context(|| format!("Cannot back up <{}>", path.display()))?;
    }
    paths::write_file(path, &document.to_string())?;
    Ok(vec![format!("Updated <{}>", paths::display(path))])
}

fn gemini_file() -> Result<PathBuf> {
    Ok(Host::Gemini.home()?.join("settings.json"))
}

fn gemini_items(settings: &JsonSettings) -> Option<Vec<String>> {
    let array = settings.get(&["ui", "footer", "items"])?.as_array()?;
    Some(array.iter().filter_map(|value| value.as_str().map(str::to_string)).collect())
}
