//! Tells plugin agents to set the issue they work on and the worktree their
//! work happens in, through a marked block in their global instructions.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::Host;
use crate::issue;
use crate::paths;
use crate::worktree;

const BEGIN: &str = "<!-- statusmaxxx:begin -->";
const END: &str = "<!-- statusmaxxx:end -->";

/// Plugin agents only: command agents hear it from their session-start hook, and
/// built-in agents cannot show the issue.
fn file(host: Host) -> Result<Option<PathBuf>> {
    Ok(match host {
        Host::Amp | Host::Pi | Host::Opencode => Some(host.home()?.join("AGENTS.md")),
        Host::Claude | Host::Cursor | Host::Qwen | Host::Droid | Host::Copilot | Host::Codex | Host::Gemini => None,
    })
}

pub fn install(host: Host) -> Result<Vec<String>> {
    let Some(path) = file(host)? else {
        return Ok(vec![]);
    };
    paths::write_file(&path, &with_block(&read(&path)?)?)?;
    Ok(vec![format!("Wrote the status line instructions to <{}>", paths::display(&path))])
}

pub fn uninstall(host: Host) -> Result<Vec<String>> {
    let Some(path) = file(host)? else {
        return Ok(vec![]);
    };
    let Some(remaining) = without_block(&read(&path)?)? else {
        return Ok(vec![]);
    };
    if remaining.trim().is_empty() {
        fs::remove_file(&path).with_context(|| format!("Cannot remove <{}>", path.display()))?;
    } else {
        paths::write_file(&path, &remaining)?;
    }
    Ok(vec![format!("Removed the status line instructions from <{}>", paths::display(&path))])
}

fn read(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
    }
}

/// The static block cannot know the session's id, so it tells the agent to
/// substitute the one it reports in its status line payload.
const AGENT_SESSION_FLAG: &str = " --session <the session id you send in your status line payload>";

fn block() -> String {
    format!(
        "{BEGIN}\n## Status line\n\n{}\n\n{}\n{END}\n",
        issue::how_to(AGENT_SESSION_FLAG),
        worktree::how_to(AGENT_SESSION_FLAG)
    )
}

/// Replaces our block, or appends it after a blank line.
fn with_block(text: &str) -> Result<String> {
    if let Some(remaining) = without_block(text)? {
        return with_block(&remaining);
    }
    Ok(match text {
        "" => block(),
        text if text.ends_with('\n') => format!("{text}\n{}", block()),
        text => format!("{text}\n\n{}", block()),
    })
}

/// The text without our block and the blank line before it; `None` without a block.
fn without_block(text: &str) -> Result<Option<String>> {
    let Some(start) = text.find(BEGIN) else {
        return Ok(None);
    };
    let Some(end) = text[start..].find(END).map(|offset| start + offset + END.len()) else {
        bail!("Found <{BEGIN}> without <{END}>; fix the file by hand");
    };
    let before = text[..start].strip_suffix('\n').filter(|before| before.ends_with('\n')).unwrap_or(&text[..start]);
    let after = text[end..].strip_prefix('\n').unwrap_or(&text[end..]);
    Ok(Some(format!("{before}{after}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installing_twice_then_removing_gives_back_the_file() {
        let cases = [
            ("", ""),
            ("# Rules\n", "# Rules\n"),
            ("# Rules", "# Rules\n"),
            ("# Rules\n\nMore.\n", "# Rules\n\nMore.\n"),
        ];
        for (original, restored) in cases {
            let installed = with_block(original).unwrap();
            assert_eq!(with_block(&installed).unwrap(), installed, "install is idempotent for <{original:?}>");
            assert_eq!(without_block(&installed).unwrap().as_deref(), Some(restored), "round trip of <{original:?}>");
        }
    }

    #[test]
    fn keeps_text_the_user_wrote_after_the_block() {
        let text = format!("# Rules\n\n{}\n# Later\n", block());
        assert_eq!(without_block(&text).unwrap().as_deref(), Some("# Rules\n\n# Later\n"));
    }
}
