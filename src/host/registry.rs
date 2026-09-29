//! The agent's own live-session registry, read so the line can show the name
//! other sessions message it by.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use serde::Deserialize;

/// Claude keeps one file per running session in `~/.claude/sessions/`, keyed by
/// pid, holding the session id and the messaging name peers see (`pulli-04`).
#[derive(Deserialize)]
struct RegistryEntry {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    name: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<u64>,
}

/// The freshest registry name for `session_id`; `None` when the agent keeps no
/// registry or the session is not in it. Names are re-derived on resume, so an
/// old pid's file can hold a stale name for the same id.
pub fn name(dir: &Path, session_id: &str) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for file in fs::read_dir(dir).ok()?.flatten() {
        let path = file.path();
        if path.extension() != Some(OsStr::new("json")) {
            continue;
        }
        // A corrupt file in the agent's registry is the agent's, not ours to fail on.
        let Ok(entry) = fs::read_to_string(&path)
            .and_then(|text| serde_json::from_str::<RegistryEntry>(&text).map_err(|error| error.into()))
        else {
            continue;
        };
        if entry.session_id.as_deref() != Some(session_id) {
            continue;
        }
        let Some(name) = entry.name.filter(|name| !name.is_empty()) else { continue };
        let stamp = entry.updated_at.unwrap_or(0);
        if best.as_ref().is_none_or(|(best_stamp, _)| stamp > *best_stamp) {
            best = Some((stamp, name));
        }
    }
    best.map(|(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: &Path, file: &str, session_id: &str, name: &str, updated_at: u64) {
        let json = format!(r#"{{"sessionId":"{session_id}","name":"{name}","updatedAt":{updated_at}}}"#);
        fs::write(dir.join(file), json).unwrap();
    }

    #[test]
    fn the_freshest_entry_wins() {
        let dir = tempfile::tempdir().unwrap();
        // The same session id resumed under a new pid; the older file's name is stale.
        entry(dir.path(), "11.json", "abc", "pulli-16", 100);
        entry(dir.path(), "22.json", "abc", "pulli-04", 200);
        entry(dir.path(), "33.json", "other", "pulli-99", 999);
        assert_eq!(name(dir.path(), "abc").as_deref(), Some("pulli-04"));
        assert_eq!(name(dir.path(), "missing"), None);
        assert_eq!(name(dir.path().join("nope").as_path(), "abc"), None);
    }
}
