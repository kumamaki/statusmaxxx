//! Issues the agent says it is working on, kept per session in the followed
//! repository's git dir. The agent knows the issue from its tracker; the status
//! line only shows what it was told — and only the session that was told, so an
//! issue cannot leak into a session that never set it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git::Repo;
use crate::paths;

/// How an agent keeps the status line's issue current. Agents read it at
/// session start; `session_flag` is ` --session <id>` for the current session,
/// or the placeholder the instructions tell agents to substitute.
pub fn how_to(session_flag: &str) -> String {
    format!(
        "When you start work on a tracked issue (Linear, GitHub, beads…), run `statusmaxxx issue set <id> \"<title>\" --state <state> --url <url>{session_flag}`; fields left out keep their value. `statusmaxxx issue clear{session_flag}` when the work lands."
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Issue {
    /// `ENG-42 "Fix auth" (In Progress)`
    fn summary(&self) -> String {
        let mut summary = self.id.clone();
        if let Some(title) = &self.title {
            summary.push_str(&format!(" \"{title}\""));
        }
        if let Some(state) = &self.state {
            summary.push_str(&format!(" ({state})"));
        }
        summary
    }
}

/// `session-id` names this session's file; `None` is the shared slot sessions
/// without an id, and humans running the command by hand, all read and write.
fn file(git_dir: &Path, session_id: Option<&str>) -> PathBuf {
    let dir = git_dir.join("statusmaxxx");
    match session_id {
        Some(id) => dir.join("issues").join(format!("{}.json", paths::sanitize(id))),
        None => dir.join("issues.json"),
    }
}

/// The issue list of one session in one worktree.
pub struct Issues {
    file: PathBuf,
    pub list: Vec<Issue>,
}

impl Issues {
    /// Inside the followed worktree's git dir: never committed, and each linked
    /// worktree has its own. Sessions only see their own list — never another's.
    pub fn of(repo: &Repo, session_id: Option<&str>) -> Result<Self> {
        let file = file(&repo.git_dir, session_id);
        Self::read(file)
    }

    /// Every slot's issues — `default` first, then one per session — for
    /// `issue show`, which answers "what is set here" for a human.
    pub fn all(repo: &Repo) -> Result<Vec<(String, Self)>> {
        let mut slots = Vec::new();
        let default = Self::of(repo, None)?;
        if !default.list.is_empty() {
            slots.push(("default".to_string(), default));
        }
        let sessions = repo.git_dir.join("statusmaxxx").join("issues");
        let entries = match fs::read_dir(&sessions) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(slots),
            Err(error) => return Err(error).with_context(|| format!("Cannot read <{}>", sessions.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension() != Some(std::ffi::OsStr::new("json")) {
                continue;
            }
            let session = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let issues = Self::read(path)?;
            if !issues.list.is_empty() {
                slots.push((session, issues));
            }
        }
        Ok(slots)
    }

    fn read(file: PathBuf) -> Result<Self> {
        let list = match fs::read_to_string(&file) {
            Ok(contents) => {
                serde_json::from_str(&contents).with_context(|| format!("<{}> is corrupt", file.display()))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error).with_context(|| format!("Cannot read <{}>", file.display())),
        };
        Ok(Self { file, list })
    }

    /// Makes `issue` the only one, keeping what was known about it.
    pub fn set(&mut self, issue: Issue) {
        let issue = self.merged(issue);
        self.list = vec![issue];
    }

    /// Adds `issue`, or updates the one with the same id.
    pub fn add(&mut self, issue: Issue) {
        let issue = self.merged(issue);
        match self.list.iter_mut().find(|existing| existing.id == issue.id) {
            Some(existing) => *existing = issue,
            None => self.list.push(issue),
        }
    }

    /// Fields left out of an update keep their stored value, so a state change needs only `--state`.
    fn merged(&self, update: Issue) -> Issue {
        match self.list.iter().find(|existing| existing.id == update.id) {
            Some(existing) => Issue {
                title: update.title.or_else(|| existing.title.clone()),
                state: update.state.or_else(|| existing.state.clone()),
                url: update.url.or_else(|| existing.url.clone()),
                id: update.id,
            },
            None => update,
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let before = self.list.len();
        self.list.retain(|issue| issue.id != id);
        if self.list.len() == before {
            bail!("<{id}> is not set");
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// What the agent is told at session start about this worktree.
    pub fn briefing(&self, session_flag: &str) -> String {
        if self.list.is_empty() {
            return format!("statusmaxxx: no issue is set for this session. {}", how_to(session_flag));
        }
        let shown: Vec<String> = self.list.iter().map(Issue::summary).collect();
        format!("statusmaxxx: this session's status line shows {}. {}", shown.join(", "), how_to(session_flag))
    }

    pub fn save(&self) -> Result<()> {
        if self.list.is_empty() {
            return match fs::remove_file(&self.file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(error).with_context(|| format!("Cannot remove <{}>", self.file.display()))
                }
                _ => Ok(()),
            };
        }
        paths::write_atomically(&self.file, &serde_json::to_string_pretty(&self.list)?)
    }
}

/// Issue files of sessions that have been dead a while; run at session start.
pub fn prune_stale(git_dir: &Path) -> Result<()> {
    paths::prune_stale(&git_dir.join("statusmaxxx").join("issues"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &str, title: Option<&str>, state: Option<&str>) -> Issue {
        Issue { id: id.into(), title: title.map(Into::into), state: state.map(Into::into), url: None }
    }

    #[test]
    fn each_session_owns_a_file_and_the_shared_slot_is_legacy() {
        let git_dir = Path::new("/repo/.git");
        assert_eq!(file(git_dir, None), git_dir.join("statusmaxxx/issues.json"));
        assert_eq!(file(git_dir, Some("abc-1")), git_dir.join("statusmaxxx/issues/abc-1.json"));
        assert_eq!(file(git_dir, Some("a/b c")), git_dir.join("statusmaxxx/issues/a_b_c.json"));
    }

    #[test]
    fn updates_keep_the_fields_they_leave_out() {
        let stored = vec![issue("ENG-1", Some("Fix auth"), Some("Todo")), issue("ENG-2", None, None)];
        let mut added = Issues { file: PathBuf::new(), list: stored.clone() };
        added.add(issue("ENG-1", None, Some("In Review")));
        added.add(issue("ENG-3", None, None));
        assert_eq!(
            added.list,
            [
                issue("ENG-1", Some("Fix auth"), Some("In Review")),
                issue("ENG-2", None, None),
                issue("ENG-3", None, None)
            ]
        );

        let mut set = Issues { file: PathBuf::new(), list: stored };
        set.set(issue("ENG-1", None, Some("Done")));
        assert_eq!(set.list, [issue("ENG-1", Some("Fix auth"), Some("Done"))]);
    }
}
