use std::cell::OnceCell;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::git::{Head, Repo};
use crate::linear;
use crate::paths;
use crate::payload::Session;
use crate::theme::Role;

const TITLE_LIMIT: usize = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Segment {
    Directory,
    Worktree,
    Git,
    Linear,
    Model,
    Context,
    Cost,
}

impl Segment {
    pub const ALL: [Segment; 7] = [
        Segment::Directory,
        Segment::Worktree,
        Segment::Git,
        Segment::Linear,
        Segment::Model,
        Segment::Context,
        Segment::Cost,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Segment::Directory => "directory",
            Segment::Worktree => "worktree",
            Segment::Git => "git",
            Segment::Linear => "linear",
            Segment::Model => "model",
            Segment::Context => "context",
            Segment::Cost => "cost",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Segment::Directory => "Working directory, relative to the repository",
            Segment::Worktree => "Repository, plus the linked worktree you are in",
            Segment::Git => "Branch and number of changed files",
            Segment::Linear => "Linear issue of this branch and your other started issues",
            Segment::Model => "Model the agent reports",
            Segment::Context => "Context window used",
            Segment::Cost => "Session cost the agent reports",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Segment::Directory => "\u{f07b}",
            Segment::Worktree => "\u{f401}",
            Segment::Git => "\u{e725}",
            Segment::Linear => "\u{f41b}",
            Segment::Model => "\u{f06a9}",
            Segment::Context => "\u{f200}",
            Segment::Cost => "",
        }
    }
}

const LINKED_WORKTREE_ICON: &str = "\u{f1bb}";

/// A run of text in one role. A segment renders to zero pieces when it has nothing to say.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub text: String,
    pub role: Role,
    pub url: Option<String>,
}

impl Piece {
    fn new(role: Role, text: impl Into<String>) -> Self {
        Self { text: text.into(), role, url: None }
    }
}

/// Everything segments read. Git and Linear are loaded on first use so a line
/// without those segments never pays for them.
pub struct Sources<'a> {
    session: &'a Session,
    icons: bool,
    repo: OnceCell<Result<Option<Repo>, String>>,
    linear: OnceCell<Result<Option<linear::Cache>, String>>,
}

impl<'a> Sources<'a> {
    pub fn new(session: &'a Session, icons: bool) -> Self {
        Self { session, icons, repo: OnceCell::new(), linear: OnceCell::new() }
    }

    fn repo(&self) -> Result<Option<&Repo>> {
        self.repo
            .get_or_init(|| Repo::discover(&self.session.cwd).map_err(|error| format!("{error:#}")))
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow!("{error}"))
    }

    fn linear(&self) -> Result<Option<&linear::Cache>> {
        self.linear
            .get_or_init(|| linear::Cache::load().map_err(|error| format!("{error:#}")))
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow!("{error}"))
    }

    fn label(&self, segment: Segment, text: &str) -> String {
        self.labeled(segment.icon(), text)
    }

    fn labeled(&self, icon: &str, text: &str) -> String {
        if self.icons && !icon.is_empty() { format!("{icon} {text}") } else { text.to_string() }
    }
}

impl Segment {
    pub fn render(self, sources: &Sources) -> Result<Vec<Piece>> {
        let session = sources.session;
        Ok(match self {
            Segment::Directory => {
                vec![Piece::new(Role::Path, sources.label(self, &directory(&session.cwd, sources.repo()?)))]
            }
            Segment::Worktree => match sources.repo()? {
                Some(repo) => worktree(repo, sources),
                None => vec![],
            },
            Segment::Git => match sources.repo()? {
                Some(repo) => git(repo, sources),
                None => vec![],
            },
            Segment::Linear => linear_issues(sources)?,
            Segment::Model => {
                session.model.iter().map(|model| Piece::new(Role::Model, sources.label(self, model))).collect()
            }
            Segment::Context => session
                .context_used_percent
                .map(|percent| {
                    let role = if percent >= 80.0 { Role::ContextHigh } else { Role::Context };
                    Piece::new(role, sources.label(self, &format!("{percent:.0}%")))
                })
                .into_iter()
                .collect(),
            Segment::Cost => {
                session.cost_usd.map(|cost| Piece::new(Role::Cost, format!("${cost:.2}"))).into_iter().collect()
            }
        })
    }
}

/// `app/src/ui` inside a repository, `~/notes` elsewhere; long paths keep their tail.
fn directory(cwd: &Path, repo: Option<&Repo>) -> String {
    let full = match repo {
        Some(repo) => match cwd.strip_prefix(&repo.root) {
            Ok(relative) if relative.as_os_str().is_empty() => repo.name.clone(),
            Ok(relative) => format!("{}/{}", repo.name, relative.display()),
            Err(_) => paths::display(cwd),
        },
        None => paths::display(cwd),
    };
    let parts: Vec<&str> = full.split('/').collect();
    if parts.len() > 3 { format!("…/{}", parts[parts.len() - 3..].join("/")) } else { full }
}

fn worktree(repo: &Repo, sources: &Sources) -> Vec<Piece> {
    match &repo.worktree {
        Some(worktree) => vec![
            Piece::new(Role::Worktree, sources.labeled(LINKED_WORKTREE_ICON, &repo.name)),
            Piece::new(Role::Muted, ":"),
            Piece::new(Role::Worktree, worktree.as_str()),
        ],
        None => vec![Piece::new(Role::Worktree, sources.label(Segment::Worktree, &repo.name))],
    }
}

fn git(repo: &Repo, sources: &Sources) -> Vec<Piece> {
    let head = match &repo.head {
        Head::Branch(name) => name.clone(),
        Head::Detached(oid) => format!("@{oid}"),
    };
    let status = if repo.changed_files == 0 {
        Piece::new(Role::Clean, " ✓")
    } else {
        Piece::new(Role::Dirty, format!(" ±{}", repo.changed_files))
    };
    vec![Piece::new(Role::Branch, sources.label(Segment::Git, &head)), status]
}

fn linear_issues(sources: &Sources) -> Result<Vec<Piece>> {
    // No cache means Linear was never set up; that is not worth a warning on every line.
    let Some(cache) = sources.linear()? else {
        return Ok(vec![]);
    };
    let branch_key = sources.repo()?.and_then(|repo| repo.head.branch()).and_then(|branch| cache.issue_key(branch));
    let others: Vec<&str> = cache
        .started
        .iter()
        .map(|issue| issue.identifier.as_str())
        .filter(|identifier| Some(*identifier) != branch_key.as_deref())
        .collect();

    let mut pieces = Vec::new();
    match &branch_key {
        Some(key) => {
            linear::record_seen(key)?;
            match cache.issue(key) {
                Some(issue) => {
                    let text = format!("{} {}", issue.identifier, truncate(&issue.title, TITLE_LIMIT));
                    pieces.push(Piece {
                        url: Some(issue.url.clone()),
                        ..Piece::new(Role::Issue, sources.label(Segment::Linear, &text))
                    });
                    pieces.push(Piece::new(Role::Muted, format!(" ({})", issue.state)));
                }
                // Seen for the first time: the next refresh fills in the title.
                None => pieces.push(Piece::new(Role::Issue, sources.label(Segment::Linear, key))),
            }
            if !others.is_empty() {
                pieces.push(Piece::new(Role::Muted, format!(" +{}", others.len())));
            }
        }
        None if !others.is_empty() => {
            let shown = others.iter().take(2).copied().collect::<Vec<_>>().join(" ");
            pieces.push(Piece::new(Role::Issue, sources.label(Segment::Linear, &shown)));
            if others.len() > 2 {
                pieces.push(Piece::new(Role::Muted, format!(" +{}", others.len() - 2)));
            }
        }
        None => {}
    }
    Ok(pieces)
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit - 1).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn repo(worktree: Option<&str>) -> Repo {
        Repo {
            root: PathBuf::from("/work/app"),
            name: "app".into(),
            worktree: worktree.map(str::to_string),
            head: Head::Branch("main".into()),
            changed_files: 0,
        }
    }

    #[test]
    fn directories_are_repository_relative_and_keep_their_tail() {
        let repo = repo(None);
        assert_eq!(directory(Path::new("/work/app"), Some(&repo)), "app");
        assert_eq!(directory(Path::new("/work/app/src"), Some(&repo)), "app/src");
        assert_eq!(directory(Path::new("/work/app/src/ui/button"), Some(&repo)), "…/src/ui/button");
    }

    #[test]
    fn linked_worktrees_show_their_folder_after_the_repository() {
        let session =
            Session { cwd: PathBuf::from("/work/app"), model: None, context_used_percent: None, cost_usd: None };
        let sources = Sources::new(&session, false);
        let text: String = worktree(&repo(Some("app-auth")), &sources).into_iter().map(|piece| piece.text).collect();
        assert_eq!(text, "app:app-auth");
        let text: String = worktree(&repo(None), &sources).into_iter().map(|piece| piece.text).collect();
        assert_eq!(text, "app");
    }
}
