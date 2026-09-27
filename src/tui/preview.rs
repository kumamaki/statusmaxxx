use std::fs;

use ansi_to_tui::IntoText;
use anyhow::Result;
use ratatui::text::{Line, Text};

use super::card;
use crate::config::Config;
use crate::host::{self, Host, Output, Tier};
use crate::payload::{Payload, Session};
use crate::{paths, render};

/// What `host` would show right now, from the session it last sent or `sample`.
pub fn line(host: Host, config: &Config, sample: &Session) -> Line<'static> {
    match last_session(host) {
        Ok(Some(session)) => render_line(host, config, &session),
        Ok(None) => render_line(host, config, sample),
        Err(error) => Line::from(card::focus(format!("Last session is unreadable: {error:#}"))),
    }
}

/// Stands in for agents that have not rendered yet.
pub fn sample_session() -> Result<Session> {
    Ok(Session {
        cwd: std::env::current_dir()?,
        model: Some("Opus".into()),
        context_used_percent: Some(42.0),
        cost_usd: Some(1.23),
    })
}

/// Built-in agents draw their own items, so their preview names the item ids.
fn render_line(host: Host, config: &Config, session: &Session) -> Line<'static> {
    if host.tier() == Tier::BuiltIn {
        let items: Vec<&str> =
            config.segments_for(host).iter().filter_map(|segment| host::builtin_item(host, *segment)).collect();
        return Line::from(card::muted(items.join(" · ")));
    }
    let segments = render::segments(host, config, session);
    match host.output() {
        Output::Ansi { .. } => render::ansi(&segments, config, false)
            .into_text()
            .map(|text: Text| text.lines.into_iter().next().unwrap_or_default())
            .unwrap_or_else(|error| Line::from(card::focus(format!("Preview failed: {error}")))),
        // Plugin agents show plain text in their own colors.
        Output::Json => Line::from(card::text(render::plain(&segments, &config.separator))),
    }
}

fn last_session(host: Host) -> Result<Option<Session>> {
    let path = paths::last_payload(host)?;
    match fs::read_to_string(&path) {
        Ok(contents) => Ok(Some(Payload::parse(&contents)?.into_session()?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
