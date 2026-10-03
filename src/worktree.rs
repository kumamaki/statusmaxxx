//! The worktree the agent says it is working in, kept per session. Agents often
//! run in the main checkout while their work happens in a linked worktree; the
//! declared path redirects every repository segment to where the work is.
//! A declaration belongs to one session, so a session that ends cannot leave
//! its worktree behind for the next session to inherit.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::paths;

/// How an agent points the status line at its worktree. Agents read it at
/// session start; `session_flag` is ` --session <id>` for the current session,
/// or the placeholder the instructions tell agents to substitute.
pub fn how_to(session_flag: &str) -> String {
    format!(
        "When your work happens in a linked worktree, run `statusmaxxx worktree set <path>{session_flag}` so the line follows it; `statusmaxxx worktree clear{session_flag}` when the work lands."
    )
}

/// `session-id` names this session's file; `None` is the shared slot sessions
/// without an id, and humans running the command by hand, all read and write.
fn file(git_dir: &Path, session_id: Option<&str>) -> PathBuf {
    let dir = git_dir.join("statusmaxxx");
    match session_id {
        Some(id) => dir.join("worktrees").join(paths::sanitize(id)),
        None => dir.join("worktree"),
    }
}

/// The worktree declared for the session in the checkout owning `git_dir`.
/// Sessions only see their own declaration — never another session's.
pub fn declared(git_dir: &Path, session_id: Option<&str>) -> Result<Option<PathBuf>> {
    match fs::read_to_string(file(git_dir, session_id)) {
        Ok(contents) => Ok(Some(PathBuf::from(contents.trim()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Cannot read <{}>", file(git_dir, session_id).display())),
    }
}

/// Every declaration this checkout holds, named by slot: `default` plus one per
/// session. `worktree show` lists them all.
pub fn all(git_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut declarations = Vec::new();
    if let Some(path) = declared(git_dir, None)? {
        declarations.push(("default".to_string(), path));
    }
    let sessions = git_dir.join("statusmaxxx").join("worktrees");
    let entries = match fs::read_dir(&sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(declarations),
        Err(error) => return Err(error).with_context(|| format!("Cannot read <{}>", sessions.display())),
    };
    for entry in entries.flatten() {
        let session = entry.file_name().to_string_lossy().into_owned();
        if let Some(path) = declared(git_dir, Some(&session))? {
            declarations.push((session, path));
        }
    }
    Ok(declarations)
}

/// `worktree` is already canonicalized by the caller, so renders compare real directories.
pub fn set(git_dir: &Path, session_id: Option<&str>, worktree: &Path) -> Result<()> {
    paths::write_atomically(&file(git_dir, session_id), &format!("{}\n", worktree.display()))
}

/// Clearing an absent marker is fine — it is the same state as never declaring.
pub fn clear(git_dir: &Path, session_id: Option<&str>) -> Result<()> {
    match fs::remove_file(file(git_dir, session_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Cannot remove <{}>", file(git_dir, session_id).display())),
    }
}

/// Markers of sessions that have been dead a while; run at session start.
pub fn prune_stale(git_dir: &Path) -> Result<()> {
    paths::prune_stale(&git_dir.join("statusmaxxx").join("worktrees"))
}

/// What the agent is told at session start about which repository the line follows.
pub fn briefing(declared: Option<&Path>, session_flag: &str) -> String {
    match declared {
        Some(path) => {
            format!("statusmaxxx: the status line follows {}. {}", paths::display(path), how_to(session_flag))
        }
        None => format!("statusmaxxx: the status line follows this checkout's repository. {}", how_to(session_flag)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_session_owns_a_file_and_the_shared_slot_is_default() {
        let git_dir = Path::new("/repo/.git");
        assert_eq!(file(git_dir, None), git_dir.join("statusmaxxx/worktree"));
        assert_eq!(file(git_dir, Some("abc-1")), git_dir.join("statusmaxxx/worktrees/abc-1"));
        assert_eq!(file(git_dir, Some("a/b c")), git_dir.join("statusmaxxx/worktrees/a_b_c"));
    }
}
