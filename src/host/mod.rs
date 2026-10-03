//! Agents we draw into. Each has one integration tier:
//! - command: the agent runs `statusmaxxx render` with session JSON on stdin;
//! - plugin: a generated shim inside the agent calls `render` and shows the text;
//! - built-in: no custom text exists, so we choose and order the agent's own items.

mod builtin;
mod command;
pub mod hook;
mod instructions;
mod plugin;
mod registry;
mod replaced;
pub mod settings;
mod wrapper;

pub use builtin::item as builtin_item;

use std::fmt;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::paths;
use crate::segment::Segment;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Host {
    Claude,
    Cursor,
    Qwen,
    Droid,
    Copilot,
    Amp,
    Pi,
    Opencode,
    Codex,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Command,
    Plugin,
    BuiltIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Ansi {
        hyperlinks: bool,
    },
    /// `{"text": …, "url": …}` for plugin shims. `ansi` when the agent renders escapes
    /// in its status area, colors and OSC 8 links alike (pi does; Amp prints them literally).
    Json {
        ansi: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallState {
    Installed,
    NotInstalled,
    /// Another status line occupies the slot; installing replaces it.
    Occupied(String),
}

impl Host {
    pub const ALL: [Host; 10] = [
        Host::Claude,
        Host::Cursor,
        Host::Qwen,
        Host::Droid,
        Host::Copilot,
        Host::Amp,
        Host::Pi,
        Host::Opencode,
        Host::Codex,
        Host::Gemini,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Host::Claude => "claude",
            Host::Cursor => "cursor",
            Host::Qwen => "qwen",
            Host::Droid => "droid",
            Host::Copilot => "copilot",
            Host::Amp => "amp",
            Host::Pi => "pi",
            Host::Opencode => "opencode",
            Host::Codex => "codex",
            Host::Gemini => "gemini",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Host::Claude => "Claude Code",
            Host::Cursor => "Cursor CLI",
            Host::Qwen => "Qwen Code",
            Host::Droid => "Factory Droid",
            Host::Copilot => "Copilot CLI",
            Host::Amp => "Amp",
            Host::Pi => "pi",
            Host::Opencode => "OpenCode",
            Host::Codex => "Codex CLI",
            Host::Gemini => "Gemini CLI",
        }
    }

    pub fn tier(self) -> Tier {
        match self {
            Host::Claude | Host::Cursor | Host::Qwen | Host::Droid | Host::Copilot => Tier::Command,
            Host::Amp | Host::Pi | Host::Opencode => Tier::Plugin,
            Host::Codex | Host::Gemini => Tier::BuiltIn,
        }
    }

    pub fn output(self) -> Output {
        match self.tier() {
            // Claude Code documents OSC 8 links. Droid strips them (checked live); others could print garbage.
            Tier::Command => Output::Ansi { hyperlinks: self == Host::Claude },
            Tier::Plugin | Tier::BuiltIn => Output::Json { ansi: self == Host::Pi },
        }
    }

    /// What the user should know about this integration before installing it.
    pub fn note(self) -> &'static str {
        match self {
            Host::Claude => "Runs as the statusLine command; issue links are clickable.",
            Host::Cursor => "Runs as the statusLine command, which replaces Cursor's own footer.",
            Host::Qwen => "Runs as ui.statusLine with the agent's colors respected.",
            Host::Droid => "Runs as the statusLine command. Droid sends no cost, so that segment stays empty.",
            Host::Copilot => {
                "Runs as the statusLine command. Copilot does not document its session JSON, so segments it does not send stay empty."
            }
            Host::Amp => "Plugin on Amp's experimental status item API, which Amp may change; CLI only.",
            Host::Pi => "Extension that adds a status entry to pi's footer.",
            Host::Opencode => {
                "TUI plugin in the app_bottom slot. OpenCode documents this API only in its repository spec."
            }
            Host::Codex | Host::Gemini => {
                "Only the agent's own items can show, so worktree and issue are not available."
            }
        }
    }

    /// Segments the agent can show. Repository segments work everywhere we render;
    /// session segments need the agent, or its shim, to send that field.
    pub fn supports(self, segment: Segment) -> bool {
        if self.tier() == Tier::BuiltIn {
            return builtin::item(self, segment).is_some();
        }
        match segment {
            Segment::Directory | Segment::Worktree | Segment::Branch | Segment::Changes | Segment::Issue => true,
            // Status lines send a session id, and the shims read theirs from the
            // plugin API (Amp's active thread, pi's session manager, OpenCode's route).
            Segment::Session => true,
            // The Amp and OpenCode shims only know the workspace folder.
            Segment::Model => !matches!(self, Host::Amp | Host::Opencode),
            Segment::Context => {
                matches!(self, Host::Claude | Host::Cursor | Host::Qwen | Host::Droid | Host::Copilot | Host::Pi)
            }
            // Copilot's payload is undocumented; its maintainers say it carries cost.
            Segment::Cost => matches!(self, Host::Claude | Host::Copilot),
        }
    }

    /// The status line payload carries a session id we can key per-session state
    /// (declared worktree, issues) to. Verified live for command agents; the
    /// shims send it by construction. Agents that do not — or are unverified —
    /// share the default slot, so their briefing leaves `--session` out.
    pub fn reports_session_id(self) -> bool {
        match self {
            Host::Claude | Host::Cursor | Host::Droid | Host::Amp | Host::Pi | Host::Opencode => true,
            Host::Qwen | Host::Copilot | Host::Codex | Host::Gemini => false,
        }
    }

    /// The session's name in the agent's own registry, when it keeps one.
    /// Claude's registry holds the messaging name other sessions reach it by.
    pub fn session_name(self, session_id: &str) -> Option<String> {
        match self {
            Host::Claude => registry::name(&self.home().ok()?.join("sessions"), session_id),
            _ => None,
        }
    }

    /// The agent's config home; its existence is how we detect the agent.
    pub fn home(self) -> Result<PathBuf> {
        match self {
            Host::Claude => paths::env_or_home("CLAUDE_CONFIG_DIR", ".claude"),
            Host::Cursor => Ok(paths::home()?.join(".cursor")),
            Host::Qwen => Ok(paths::home()?.join(".qwen")),
            Host::Droid => Ok(paths::home()?.join(".factory")),
            Host::Copilot => paths::env_or_home("COPILOT_HOME", ".copilot"),
            Host::Amp => Ok(paths::xdg_config_home()?.join("amp")),
            Host::Pi => Ok(paths::home()?.join(".pi").join("agent")),
            Host::Opencode => Ok(paths::xdg_config_home()?.join("opencode")),
            Host::Codex => paths::env_or_home("CODEX_HOME", ".codex"),
            Host::Gemini => Ok(paths::home()?.join(".gemini")),
        }
    }

    pub fn detected(self) -> Result<bool> {
        Ok(self.home()?.is_dir())
    }

    pub fn state(self, config: &Config) -> Result<InstallState> {
        match self.tier() {
            Tier::Command => command::state(self),
            Tier::Plugin => plugin::state(self),
            Tier::BuiltIn => builtin::state(self, config),
        }
    }

    /// Returns one line per file written.
    pub fn install(self, config: &Config) -> Result<Vec<String>> {
        let mut report = match self.tier() {
            Tier::Command => command::install(self)?,
            Tier::Plugin => plugin::install(self)?,
            Tier::BuiltIn => builtin::install(self, config)?,
        };
        report.extend(hook::install(self)?);
        report.extend(instructions::install(self)?);
        Ok(report)
    }

    /// Built-in agents keep their own copy of the segment list, so a config change is
    /// written into it; the others read the config on every render. Returns the items
    /// written, when it wrote any.
    pub fn sync(self, old: &Config, new: &Config) -> Result<Option<Vec<&'static str>>> {
        match self.tier() {
            Tier::BuiltIn => builtin::sync(self, old, new),
            Tier::Command | Tier::Plugin => Ok(None),
        }
    }

    /// `config` tells built-in agents whether their current items are ours to remove.
    pub fn uninstall(self, config: &Config) -> Result<Vec<String>> {
        let mut report = match self.tier() {
            Tier::Command => command::uninstall(self)?,
            Tier::Plugin => plugin::uninstall(self)?,
            Tier::BuiltIn => builtin::uninstall(self, config)?,
        };
        report.extend(hook::uninstall(self)?);
        report.extend(instructions::uninstall(self)?);
        Ok(report)
    }
}

impl fmt::Display for Host {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id())
    }
}

/// Single-quotes `text` for `/bin/sh`.
pub(crate) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}
