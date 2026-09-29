//! The worktree the agent says it is working in, kept per checkout. Agents often
//! run in the main checkout while their work happens in a linked worktree; the
//! declared path redirects every repository segment to where the work is.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::paths;

/// How an agent points the status line at its worktree. Agents read it at session start.
pub const HOW_TO: &str = "When your work happens in a linked worktree, run `statusmaxxx worktree set <path>` so the line follows it; `statusmaxxx worktree clear` when the work lands.";

/// Beside `issues.json` in the checkout's own git dir: never committed, and
/// each linked worktree keeps its own declaration.
fn file(git_dir: &Path) -> PathBuf {
    git_dir.join("statusmaxxx").join("worktree")
}

/// The worktree declared for the checkout owning `git_dir`; `None` until set.
pub fn declared(git_dir: &Path) -> Result<Option<PathBuf>> {
    match fs::read_to_string(file(git_dir)) {
        Ok(contents) => Ok(Some(PathBuf::from(contents.trim()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Cannot read <{}>", file(git_dir).display())),
    }
}

/// `worktree` is already canonicalized by the caller, so renders compare real directories.
pub fn set(git_dir: &Path, worktree: &Path) -> Result<()> {
    paths::write_atomically(&file(git_dir), &format!("{}\n", worktree.display()))
}

/// Clearing an absent marker is fine — it is the same state as never declaring.
pub fn clear(git_dir: &Path) -> Result<()> {
    match fs::remove_file(file(git_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Cannot remove <{}>", file(git_dir).display())),
    }
}

/// What the agent is told at session start about which repository the line follows.
pub fn briefing(declared: Option<&Path>) -> String {
    match declared {
        Some(path) => format!("statusmaxxx: the status line follows {}. {HOW_TO}", paths::display(path)),
        None => format!("statusmaxxx: the status line follows this checkout's repository. {HOW_TO}"),
    }
}
