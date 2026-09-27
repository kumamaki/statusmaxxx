use ansi_to_tui::IntoText;
use anyhow::Result;
use ratatui::text::{Line, Text};

use super::card;
use crate::config::Config;
use crate::host::{self, Host, Output, Tier};
use crate::payload::Session;
use crate::render;
use crate::segment::Segment;

/// The same session for every agent, so previews differ only by the agent.
pub fn sample_session() -> Result<Session> {
    Ok(Session {
        cwd: std::env::current_dir()?,
        model: Some("Opus".into()),
        context_used_percent: Some(42.0),
        cost_usd: Some(1.23),
    })
}

/// What `host` would show for the sample session.
pub fn line(host: Host, config: &Config, sample: &Session) -> Line<'static> {
    if host.tier() == Tier::BuiltIn {
        // Built-in agents draw their own items, so the preview names the item ids.
        let items: Vec<&str> =
            config.segments_for(host).iter().filter_map(|segment| host::builtin_item(host, *segment)).collect();
        return Line::from(card::muted(items.join(" · ")));
    }
    let segments = render::segments(host, config, &as_sent_by(host, sample));
    match host.output() {
        Output::Ansi { .. } => render::ansi(&segments, config, false)
            .into_text()
            .map(|text: Text| text.lines.into_iter().next().unwrap_or_default())
            .unwrap_or_else(|error| Line::from(card::focus(format!("Preview failed: {error}")))),
        // Plugin agents show plain text in their own colors.
        Output::Json => Line::from(card::text(render::plain(&segments, &config.separator))),
    }
}

/// Segments `host` is set to show but cannot.
pub fn missing(host: Host, config: &Config) -> Vec<Segment> {
    config.segments_for(host).iter().copied().filter(|segment| !host.supports(*segment)).collect()
}

/// The sample without the fields `host` never sends.
fn as_sent_by(host: Host, sample: &Session) -> Session {
    Session {
        cwd: sample.cwd.clone(),
        model: sample.model.clone().filter(|_| host.supports(Segment::Model)),
        context_used_percent: sample.context_used_percent.filter(|_| host.supports(Segment::Context)),
        cost_usd: sample.cost_usd.filter(|_| host.supports(Segment::Cost)),
    }
}
