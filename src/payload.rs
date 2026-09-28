use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// What an agent tells us about its session, from its status line or its hooks.
/// Hosts send the Claude Code shape (the plugin shims by construction) and omit
/// what they do not track, so every field is optional.
#[derive(Debug, Default, Deserialize)]
pub struct Payload {
    cwd: Option<PathBuf>,
    workspace: Option<Workspace>,
    /// Cursor's hooks send the workspace folders instead of a cwd.
    workspace_roots: Option<Vec<PathBuf>>,
    model: Option<Model>,
    context_window: Option<ContextWindow>,
    /// Droid's shape for the context window; `null` before the first reply.
    context: Option<DroidContext>,
    cost: Option<Cost>,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    current_dir: Option<PathBuf>,
}

/// Status lines send an object; Cursor's hooks send the bare model name.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Model {
    Named { display_name: Option<String> },
    Name(String),
}

impl Model {
    fn into_name(self) -> Option<String> {
        match self {
            Model::Named { display_name } => display_name,
            Model::Name(name) => Some(name),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ContextWindow {
    used_percentage: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct DroidContext {
    percentage: Option<f64>,
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
        let cwd = self
            .cwd
            .or(self.workspace.and_then(|workspace| workspace.current_dir))
            .or(self.workspace_roots.and_then(|roots| roots.into_iter().next()));
        let cwd = match cwd {
            Some(cwd) => cwd,
            // Agents spawn the command inside the session directory, so the
            // process cwd is the session cwd when the payload leaves it out.
            None => env::current_dir().context("Cannot resolve the working directory")?,
        };
        Ok(Session {
            cwd,
            model: self.model.and_then(Model::into_name),
            context_used_percent: self
                .context_window
                .and_then(|window| window.used_percentage)
                .or(self.context.and_then(|context| context.percentage)),
            cost_usd: self.cost.and_then(|cost| cost.total_cost_usd),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_agent_shape() {
        let cases = [
            (
                r#"{"workspace":{"current_dir":"/repo"},"model":{"display_name":"Opus"},
                    "context_window":{"used_percentage":42.5},"cost":{"total_cost_usd":1.5}}"#,
                ("/repo", Some("Opus"), Some(42.5), Some(1.5)),
            ),
            // Droid 0.228: its own context object, recorded from a live session.
            (
                r#"{"cwd":"/repo","model":{"id":"glm-5.3-flash","display_name":"GLM-5.3-Flash"},
                    "context":{"token_limit":250000,"percentage":18,"display":"18%"}}"#,
                ("/repo", Some("GLM-5.3-Flash"), Some(18.0), None),
            ),
            // Cursor's hooks: workspace folders and a bare model name.
            (
                r#"{"workspace_roots":["/repo"],"model":"grok-4","transcript_path":null}"#,
                ("/repo", Some("grok-4"), None, None),
            ),
        ];
        for (json, (cwd, model, context, cost)) in cases {
            let session = Payload::parse(json).unwrap().into_session().unwrap();
            assert_eq!(session.cwd, PathBuf::from(cwd), "cwd of <{json}>");
            assert_eq!(session.model.as_deref(), model, "model of <{json}>");
            assert_eq!(session.context_used_percent, context);
            assert_eq!(session.cost_usd, cost);
        }
    }
}
