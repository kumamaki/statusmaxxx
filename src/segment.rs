use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::git::{Head, Repo};
use crate::issue::Issues;
use crate::paths;
use crate::payload::Session;
use crate::theme::Role;

const TITLE_LIMIT: usize = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Segment {
    Directory,
    Worktree,
    Branch,
    Changes,
    Issue,
    Model,
    Context,
    Cost,
}

impl Segment {
    pub const ALL: [Segment; 8] = [
        Segment::Directory,
        Segment::Worktree,
        Segment::Branch,
        Segment::Changes,
        Segment::Issue,
        Segment::Model,
        Segment::Context,
        Segment::Cost,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Segment::Directory => "directory",
            Segment::Worktree => "worktree",
            Segment::Branch => "branch",
            Segment::Changes => "changes",
            Segment::Issue => "issue",
            Segment::Model => "model",
            Segment::Context => "context",
            Segment::Cost => "cost",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Segment::Directory => "Working directory, relative to the repository",
            Segment::Worktree => "Repository, plus the linked worktree you are in",
            Segment::Branch => "Current branch, or the commit when detached",
            Segment::Changes => "Changed files, or a check when the tree is clean",
            Segment::Issue => "Issues the agent set with `statusmaxxx issue set`",
            Segment::Model => "Model the agent reports",
            Segment::Context => "Context window used",
            Segment::Cost => "Session cost the agent reports",
        }
    }

    pub fn has_icon(self) -> bool {
        !self.icon().is_empty()
    }

    pub fn icon(self) -> &'static str {
        match self {
            Segment::Directory => "\u{f07b}",
            Segment::Worktree => "\u{f401}",
            Segment::Branch => "\u{e725}",
            // `±3` and `✓` already are the glyph.
            Segment::Changes => "",
            Segment::Issue => "\u{f41b}",
            Segment::Model => "\u{f06a9}",
            Segment::Context => "\u{f200}",
            Segment::Cost => "",
        }
    }
}

impl Segment {
    pub fn from_name(name: &str) -> Option<Segment> {
        Segment::ALL.into_iter().find(|segment| segment.name() == name)
    }

    /// Segments named in a config list. `git` names `branch` then `changes`.
    pub fn parse_names(names: &[String]) -> Result<Vec<Segment>, String> {
        let mut segments = Vec::new();
        for name in names {
            match name.as_str() {
                "git" => segments.extend([Segment::Branch, Segment::Changes]),
                name => segments.push(Segment::from_name(name).ok_or_else(|| format!("unknown segment <{name}>"))?),
            }
        }
        Ok(segments)
    }

    pub fn deserialize_list<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Segment>, D::Error> {
        let names = Vec::<String>::deserialize(deserializer)?;
        Segment::parse_names(&names).map_err(serde::de::Error::custom)
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

/// Everything segments read. Git is loaded on first use so a line without git
/// segments never pays for it.
pub struct Sources<'a> {
    session: &'a Session,
    icons: &'a BTreeSet<Segment>,
    repo: OnceCell<Result<Option<Repo>, String>>,
}

impl<'a> Sources<'a> {
    pub fn new(session: &'a Session, icons: &'a BTreeSet<Segment>) -> Self {
        Self { session, icons, repo: OnceCell::new() }
    }

    fn repo(&self) -> Result<Option<&Repo>> {
        self.repo
            .get_or_init(|| Repo::discover(&self.session.cwd).map_err(|error| format!("{error:#}")))
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow!("{error}"))
    }

    fn label(&self, segment: Segment, text: &str) -> String {
        self.labeled(segment, segment.icon(), text)
    }

    /// `icon text` when `segment` has icons on; `icon` may differ from the segment's own.
    fn labeled(&self, segment: Segment, icon: &str, text: &str) -> String {
        if self.icons.contains(&segment) && !icon.is_empty() { format!("{icon} {text}") } else { text.to_string() }
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
            Segment::Branch => match sources.repo()? {
                Some(repo) => vec![branch(repo, sources)],
                None => vec![],
            },
            Segment::Changes => match sources.repo()? {
                Some(repo) => vec![changes(repo)],
                None => vec![],
            },
            Segment::Issue => match sources.repo()? {
                Some(repo) => issues(&Issues::of(repo)?, sources),
                None => vec![],
            },
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
            Piece::new(Role::Worktree, sources.labeled(Segment::Worktree, LINKED_WORKTREE_ICON, &repo.name)),
            Piece::new(Role::Muted, ":"),
            Piece::new(Role::Worktree, worktree.as_str()),
        ],
        None => vec![Piece::new(Role::Worktree, sources.label(Segment::Worktree, &repo.name))],
    }
}

fn branch(repo: &Repo, sources: &Sources) -> Piece {
    let head = match &repo.head {
        Head::Branch(name) => name.clone(),
        Head::Detached(oid) => format!("@{oid}"),
    };
    Piece::new(Role::Branch, sources.label(Segment::Branch, &head))
}

fn changes(repo: &Repo) -> Piece {
    match repo.changed_files {
        0 => Piece::new(Role::Clean, "✓"),
        count => Piece::new(Role::Dirty, format!("±{count}")),
    }
}

/// The first issue in full, the rest by id.
fn issues(issues: &Issues, sources: &Sources) -> Vec<Piece> {
    let Some((first, rest)) = issues.list.split_first() else {
        return vec![];
    };
    let text = match &first.title {
        Some(title) => format!("{} {}", first.id, truncate(title, TITLE_LIMIT)),
        None => first.id.clone(),
    };
    let mut pieces =
        vec![Piece { url: first.url.clone(), ..Piece::new(Role::Issue, sources.label(Segment::Issue, &text)) }];
    if let Some(state) = &first.state {
        pieces.push(Piece::new(Role::Muted, format!(" ({state})")));
    }
    for issue in rest {
        pieces.push(Piece { url: issue.url.clone(), ..Piece::new(Role::Issue, format!(" {}", issue.id)) });
    }
    pieces
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
            git_dir: PathBuf::from("/work/app/.git"),
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
        let no_icons = BTreeSet::new();
        let sources = Sources::new(&session, &no_icons);
        let text: String = worktree(&repo(Some("app-auth")), &sources).into_iter().map(|piece| piece.text).collect();
        assert_eq!(text, "app:app-auth");
        let text: String = worktree(&repo(None), &sources).into_iter().map(|piece| piece.text).collect();
        assert_eq!(text, "app");
    }
}
