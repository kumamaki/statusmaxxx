use serde_json::json;

use crate::config::Config;
use crate::host::{Host, Output};
use crate::payload::Session;
use crate::segment::{Piece, Segment, Sources};
use crate::theme::{Role, Theme};

pub fn render(host: Host, config: &Config, session: &Session) -> String {
    let segments = segments(host, config, session);
    match host.output() {
        Output::Ansi { hyperlinks } => ansi(&segments, config, hyperlinks),
        Output::Json { colored } => {
            let url = segments.iter().flatten().find_map(|piece| piece.url.clone());
            let text = if colored { ansi(&segments, config, false) } else { plain(&segments, &config.separator) };
            json!({ "text": text, "url": url }).to_string()
        }
    }
}

/// The non-empty segments `host` shows, in order.
pub fn segments(host: Host, config: &Config, session: &Session) -> Vec<Vec<Piece>> {
    let sources = Sources::new(session, config.icons);
    config
        .segments_for(host)
        .iter()
        .map(|segment| render_segment(*segment, &sources))
        .filter(|pieces| !pieces.is_empty())
        .collect()
}

/// A failing segment shows up in the line instead of blanking the whole line.
fn render_segment(segment: Segment, sources: &Sources) -> Vec<Piece> {
    segment.render(sources).unwrap_or_else(|error| {
        eprintln!("[statusmaxxx] {} segment failed: {error:#}", segment.name());
        vec![Piece { text: format!("✗ {}", segment.name()), role: Role::Error, url: None }]
    })
}

pub fn ansi(segments: &[Vec<Piece>], config: &Config, hyperlinks: bool) -> String {
    let separator = config.theme.paint(Role::Muted, &config.separator);
    segments
        .iter()
        .map(|pieces| pieces.iter().map(|piece| paint(piece, config.theme, hyperlinks)).collect::<String>())
        .collect::<Vec<_>>()
        .join(&separator)
}

fn paint(piece: &Piece, theme: Theme, hyperlinks: bool) -> String {
    let painted = theme.paint(piece.role, &piece.text);
    match (&piece.url, hyperlinks) {
        (Some(url), true) => format!("\x1b]8;;{url}\x1b\\{painted}\x1b]8;;\x1b\\"),
        _ => painted,
    }
}

pub fn plain(segments: &[Vec<Piece>], separator: &str) -> String {
    segments
        .iter()
        .map(|pieces| pieces.iter().map(|piece| piece.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join(separator)
}
