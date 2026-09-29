use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::git::{Head, Repo};
use crate::issue::{Issue, Issues};
use crate::paths;
use crate::payload::Session;
use crate::theme::Role;

/// Issue titles run long; the id carries the meaning, the words are a reminder.
const TITLE_WORDS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Segment {
    Directory,
    Worktree,
    Branch,
    Changes,
    Issue,
    Session,
    Model,
    Context,
    Cost,
}

impl Segment {
    pub const ALL: [Segment; 9] = [
        Segment::Directory,
        Segment::Worktree,
        Segment::Branch,
        Segment::Changes,
        Segment::Issue,
        Segment::Session,
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
            Segment::Session => "session",
            Segment::Model => "model",
            Segment::Context => "context",
            Segment::Cost => "cost",
        }
    }

    /// How the TUI titles the segment; `name` is what the config says.
    pub fn label(self) -> &'static str {
        match self {
            Segment::Directory => "Directory",
            Segment::Worktree => "Worktree",
            Segment::Branch => "Branch",
            Segment::Changes => "Changes",
            Segment::Issue => "Current issue",
            Segment::Session => "Session",
            Segment::Model => "Model",
            Segment::Context => "Context",
            Segment::Cost => "Cost",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Segment::Directory => "Working directory, relative to the repository",
            Segment::Worktree => "Repository, plus the linked worktree you are in",
            Segment::Branch => "Current branch, or the commit when detached",
            Segment::Changes => "Changed files, or a check when the tree is clean",
            Segment::Issue => "Issues the agent set with `statusmaxxx issue set`",
            Segment::Session => "Session's name, or the short id",
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
            Segment::Session => "\u{f120}",
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

/// Everything segments read. Git and the issue file are loaded on first use so
/// a line without those segments never pays for them.
pub struct Sources<'a> {
    session: &'a Session,
    icons: &'a BTreeSet<Segment>,
    repo: OnceCell<Result<Option<Repo>, String>>,
    issues: OnceCell<Result<Vec<Issue>, String>>,
}

impl<'a> Sources<'a> {
    pub fn new(session: &'a Session, icons: &'a BTreeSet<Segment>) -> Self {
        Self { session, icons, repo: OnceCell::new(), issues: OnceCell::new() }
    }

    /// Sources that never read the disk, for a preview that looks the same wherever it runs.
    pub fn with_repo(session: &'a Session, icons: &'a BTreeSet<Segment>, repo: Repo, issues: Vec<Issue>) -> Self {
        Self { session, icons, repo: OnceCell::from(Ok(Some(repo))), issues: OnceCell::from(Ok(issues)) }
    }

    fn repo(&self) -> Result<Option<&Repo>> {
        self.repo
            .get_or_init(|| Repo::discover(&self.session.cwd).map_err(|error| format!("{error:#}")))
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow!("{error}"))
    }

    /// The worktree's issues; none outside a repository.
    fn issues(&self) -> Result<&[Issue]> {
        let repo = self.repo()?;
        self.issues
            .get_or_init(|| match repo {
                Some(repo) => Issues::of(repo).map(|issues| issues.list).map_err(|error| format!("{error:#}")),
                None => Ok(Vec::new()),
            })
            .as_deref()
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
            Segment::Issue => issues(sources.issues()?, sources),
            Segment::Session => session_label(session)
                .map(|label| Piece::new(Role::Muted, sources.label(self, &label)))
                .into_iter()
                .collect(),
            Segment::Model => session
                .model
                .iter()
                .map(|model| Piece::new(Role::Model, sources.labeled(self, model_icon(model), model)))
                .collect(),
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
fn issues(issues: &[Issue], sources: &Sources) -> Vec<Piece> {
    let Some((first, rest)) = issues.split_first() else {
        return vec![];
    };
    let text = match &first.title {
        Some(title) => format!("{} {}", first.id, first_words(title, TITLE_WORDS)),
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

/// The session's name when the agent sends one; otherwise the id's short form.
/// Session ids are UUIDs, so eight characters identify the session.
fn session_label(session: &Session) -> Option<String> {
    if let Some(name) = &session.session_name {
        return Some(first_words(name, TITLE_WORDS));
    }
    session.session_id.as_deref().map(|id| id.get(..8).unwrap_or(id).to_string())
}

/// Model-name needles → their glyphs, checked in order so a compound name like
/// `gpt-5.1-codex` takes the book rather than the OpenAI knot. Anything that
/// matches none keeps the segment's own icon.
const MODEL_ICONS: &[(&str, &str)] = &[
    ("opus", "\u{f4f5}"),      // north star
    ("sonnet", "\u{f219}"),    // diamond
    ("haiku", "\u{f06c}"),     // leaf
    ("fable", "\u{f06d3}"),    // quill
    ("mythos", "\u{ef0d}"),    // scroll
    ("claude", "\u{f069}"),    // Anthropic's starburst
    ("codex", "\u{f00bd}"),    // bound book
    ("gpt", "\u{ec81}"),       // OpenAI knot
    ("gemini", "\u{f0ae2}"),   // four-point sparkle
    ("grok", "\u{f0b05}"),     // xAI's X
    ("deepseek", "\u{f18b4}"), // whale
    ("codestral", "\u{ef16}"), // wind
    ("mistral", "\u{ef16}"),   // wind
    ("kimi", "\u{f186}"),      // moon
    ("moonshot", "\u{f186}"),
    ("copilot", "\u{ec1e}"), // copilot
    ("swe", "\u{f121}"),     // code brackets, the software engineer
    ("devin", "\u{f121}"),
    ("glm", "\u{f075}"),     // chat bubble, ChatGLM's mark
    ("llama", "\u{edfe}"),   // Meta's loop
    ("phi", "\u{f0372}"),    // Microsoft's panes
    ("qwen", "\u{f1331}"),   // a CJK ideogram
    ("cohere", "\u{f0564}"), // two shapes joined
    ("minimax", "\u{ed63}"), // chess knight — the game-tree algorithm
];

fn model_icon(model: &str) -> &'static str {
    let name = model.to_lowercase();
    MODEL_ICONS
        .iter()
        .find(|(needle, _)| name.contains(needle))
        .map(|(_, icon)| *icon)
        .unwrap_or_else(|| Segment::Model.icon())
}

fn first_words(text: &str, count: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= count { words.join(" ") } else { format!("{}…", words[..count].join(" ")) }
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
            declared: None,
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
        let session = Session {
            cwd: PathBuf::from("/work/app"),
            model: None,
            context_used_percent: None,
            cost_usd: None,
            session_id: None,
            session_name: None,
        };
        let no_icons = BTreeSet::new();
        let sources = Sources::new(&session, &no_icons);
        let text: String = worktree(&repo(Some("app-auth")), &sources).into_iter().map(|piece| piece.text).collect();
        assert_eq!(text, "app:app-auth");
        let text: String = worktree(&repo(None), &sources).into_iter().map(|piece| piece.text).collect();
        assert_eq!(text, "app");
    }

    #[test]
    fn the_session_segment_prefers_the_name_then_the_short_id() {
        let session = |id: Option<&str>, name: Option<&str>| Session {
            cwd: PathBuf::from("/work/app"),
            model: None,
            context_used_percent: None,
            cost_usd: None,
            session_id: id.map(str::to_string),
            session_name: name.map(str::to_string),
        };
        let named = session(Some("82570de0-186d-4628-9996-a5b2a03955ea"), Some("Tidy up"));
        assert_eq!(session_label(&named).as_deref(), Some("Tidy up"));
        let unnamed = session(Some("82570de0-186d-4628-9996-a5b2a03955ea"), None);
        assert_eq!(session_label(&unnamed).as_deref(), Some("82570de0"));
        let titled = session(None, Some("Claude sessions messaging each other"));
        assert_eq!(session_label(&titled).as_deref(), Some("Claude sessions messaging each…"));
        assert_eq!(session_label(&session(None, None)), None);
    }

    #[test]
    fn model_icons_match_families_and_compound_names_take_the_specific_one() {
        assert_eq!(model_icon("Sonnet 5.5"), "\u{f219}");
        assert_eq!(model_icon("gpt-5.1-codex"), "\u{f00bd}");
        assert_eq!(model_icon("[Devin] SWE 2"), "\u{f121}");
        assert_eq!(model_icon("qwen3-coder"), "\u{f1331}");
        assert_eq!(model_icon("MiniMax-M2"), "\u{ed63}");
        assert_eq!(model_icon("some-future-model"), Segment::Model.icon());
    }

    #[test]
    fn issue_titles_keep_their_first_four_words() {
        assert_eq!(first_words("Fix the auth flow", 4), "Fix the auth flow");
        assert_eq!(first_words("Point Manage payment at Suby's customer portal", 4), "Point Manage payment at…");
    }
}
