//! Issues the agent says it is working on, kept per worktree. The agent knows
//! the issue from its tracker; the status line only shows what it was told.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git::Repo;
use crate::paths;

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

/// How an agent keeps the status line's issue current. Agents read it at session start.
pub const HOW_TO: &str = "When you start work on a tracked issue (Linear, GitHub, beads, or any other tracker), run `statusmaxxx issue set <id> \"<title>\" --state \"<state>\" --url \"<issue url>\"` so the status line shows it and links to it. When the state changes, run `statusmaxxx issue set <id> --state \"<state>\"`. Once the work lands, run `statusmaxxx issue clear`.";

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

/// The issue list of one worktree.
pub struct Issues {
    file: PathBuf,
    pub list: Vec<Issue>,
}

impl Issues {
    pub fn of(repo: &Repo) -> Result<Self> {
        // Inside the worktree's git dir: never committed, and each linked worktree has its own.
        let file = repo.git_dir.join("statusmaxxx").join("issues.json");
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
            bail!("<{id}> is not set in this worktree");
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// What the agent is told at session start about this worktree.
    pub fn briefing(&self) -> String {
        if self.list.is_empty() {
            return format!("statusmaxxx: no issue is set for this worktree. {HOW_TO}");
        }
        let shown: Vec<String> = self.list.iter().map(Issue::summary).collect();
        format!("statusmaxxx: this worktree's status line shows {}. {HOW_TO}", shown.join(", "))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &str, title: Option<&str>, state: Option<&str>) -> Issue {
        Issue { id: id.into(), title: title.map(Into::into), state: state.map(Into::into), url: None }
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
