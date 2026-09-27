//! Tells the agent to set the issue it works on, through its global
//! instructions. Shared files get a marked block; agents that load a folder of
//! instruction files get a file of their own.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::Host;
use crate::paths;

const BEGIN: &str = "<!-- statusmaxxx:begin -->";
const END: &str = "<!-- statusmaxxx:end -->";
const INSTRUCTION: &str = "## Status line issue

When you start work on a tracked issue (Linear, GitHub, beads, or any other tracker), run `statusmaxxx issue set <id> \"<title>\" --state \"<state>\"` so the status line shows it. When the state changes, run `statusmaxxx issue set <id> --state \"<state>\"`. Once the work lands, run `statusmaxxx issue clear`.
";

enum Target {
    Block(PathBuf),
    OwnFile(PathBuf),
}

/// `None` where the agent has no global instructions file, or cannot show the issue.
fn target(host: Host) -> Result<Option<Target>> {
    let home = host.home()?;
    Ok(match host {
        Host::Claude => Some(Target::Block(home.join("CLAUDE.md"))),
        Host::Qwen => Some(Target::Block(home.join("QWEN.md"))),
        Host::Droid | Host::Amp | Host::Pi | Host::Opencode => Some(Target::Block(home.join("AGENTS.md"))),
        Host::Copilot => Some(Target::OwnFile(home.join("instructions").join("statusmaxxx.instructions.md"))),
        Host::Cursor | Host::Codex | Host::Gemini => None,
    })
}

pub fn install(host: Host) -> Result<Vec<String>> {
    let (path, contents) = match target(host)? {
        None => return Ok(vec![]),
        Some(Target::OwnFile(path)) => (path, format!("<!-- Managed by statusmaxxx -->\n{INSTRUCTION}")),
        Some(Target::Block(path)) => {
            let contents = with_block(&read(&path)?)?;
            (path, contents)
        }
    };
    paths::write_file(&path, &contents)?;
    Ok(vec![format!("Wrote the issue instruction to <{}>", paths::display(&path))])
}

pub fn uninstall(host: Host) -> Result<Vec<String>> {
    let path = match target(host)? {
        None => return Ok(vec![]),
        Some(Target::OwnFile(path)) if path.exists() => {
            fs::remove_file(&path).with_context(|| format!("Cannot remove <{}>", path.display()))?;
            return Ok(vec![format!("Removed <{}>", paths::display(&path))]);
        }
        Some(Target::OwnFile(_)) => return Ok(vec![]),
        Some(Target::Block(path)) => path,
    };
    let Some(remaining) = without_block(&read(&path)?)? else {
        return Ok(vec![]);
    };
    if remaining.trim().is_empty() {
        fs::remove_file(&path).with_context(|| format!("Cannot remove <{}>", path.display()))?;
    } else {
        paths::write_file(&path, &remaining)?;
    }
    Ok(vec![format!("Removed the issue instruction from <{}>", paths::display(&path))])
}

fn read(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
    }
}

fn block() -> String {
    format!("{BEGIN}\n{INSTRUCTION}{END}\n")
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
