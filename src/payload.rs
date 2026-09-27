use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// What an agent tells us about its session. Every host sends the Claude Code
/// shape (the command hosts natively, the plugin shims by construction), and
/// hosts omit what they do not track, so every field is optional.
#[derive(Debug, Default, Deserialize)]
pub struct Payload {
    cwd: Option<PathBuf>,
    workspace: Option<Workspace>,
    model: Option<Model>,
    context_window: Option<ContextWindow>,
    cost: Option<Cost>,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    current_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Model {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContextWindow {
    used_percentage: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Cost {
    total_cost_usd: Option<f64>,
}

/// The normalized session the segments render from.
#[derive(Debug, Clone)]
pub struct Session {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub context_used_percent: Option<f64>,
    pub cost_usd: Option<f64>,
}

impl Payload {
    pub fn parse(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).context("Session JSON on stdin is malformed")
    }

    pub fn into_session(self) -> Result<Session> {
        let cwd = match self.cwd.or(self.workspace.and_then(|workspace| workspace.current_dir)) {
            Some(cwd) => cwd,
            // Agents spawn the command inside the session directory, so the
            // process cwd is the session cwd when the payload leaves it out.
            None => env::current_dir().context("Cannot resolve the working directory")?,
        };
        Ok(Session {
            cwd,
            model: self.model.and_then(|model| model.display_name),
            context_used_percent: self.context_window.and_then(|window| window.used_percentage),
            cost_usd: self.cost.and_then(|cost| cost.total_cost_usd),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_claude_code_shape() {
        let payload = Payload::parse(
            r#"{"workspace":{"current_dir":"/repo"},"model":{"display_name":"Opus"},
                "context_window":{"used_percentage":42.5},"cost":{"total_cost_usd":1.5}}"#,
        )
        .unwrap();
        let session = payload.into_session().unwrap();
        assert_eq!(session.cwd, PathBuf::from("/repo"));
        assert_eq!(session.model.as_deref(), Some("Opus"));
        assert_eq!(session.context_used_percent, Some(42.5));
        assert_eq!(session.cost_usd, Some(1.5));
    }
}
