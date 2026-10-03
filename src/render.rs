use serde_json::json;

use crate::config::Config;
use crate::host::{Host, Output};
use crate::payload::Session;
use crate::segment::{Piece, Segment, Sources};
use crate::theme::{Role, Theme};

/// `problems` are failures the line must still show — a corrupt config, a
/// payload we could not read. They land as `✗` text after the segments.
pub fn render(host: Host, config: &Config, session: &Session, problems: &[String]) -> String {
    let segments = segments(config.segments_for(host), &Sources::new(session, &config.icons, config.icon_font));
    let error = problems.iter().map(|problem| format!("✗ {problem}")).collect::<Vec<_>>().join(" ");
    match host.output() {
        Output::Ansi { hyperlinks } => {
            appended(ansi(&segments, config, hyperlinks), &error, &ansi_separator(config), |text| {
                config.theme.paint(Role::Error, text)
            })
        }
        Output::Json { ansi: renders_ansi } => {
            let url = segments.iter().flat_map(|(_, pieces)| pieces).find_map(|piece| piece.url.clone());
            let text = if renders_ansi {
                appended(ansi(&segments, config, true), &error, &ansi_separator(config), |text| {
                    config.theme.paint(Role::Error, text)
                })
            } else {
                appended(plain(&segments, &config.separator), &error, &config.separator, str::to_string)
            };
            json!({ "text": text, "url": url }).to_string()
        }
    }
}

/// `line` followed by `error` as a trailing segment; `paint` styles it for the host.
fn appended(line: String, error: &str, separator: &str, paint: impl FnOnce(&str) -> String) -> String {
    if error.is_empty() {
        return line;
    }
    if line.is_empty() {
        return paint(error);
    }
    format!("{line}{separator}{}", paint(error))
}

/// The non-empty segments `list` draws, in order, each with what it drew.
pub fn segments(list: &[Segment], sources: &Sources) -> Vec<(Segment, Vec<Piece>)> {
    list.iter()
        .map(|segment| (*segment, render_segment(*segment, sources)))
        .filter(|(_, pieces)| !pieces.is_empty())
        .collect()
}

/// A failing segment shows up in the line instead of blanking the whole line.
fn render_segment(segment: Segment, sources: &Sources) -> Vec<Piece> {
    segment.render(sources).unwrap_or_else(|error| {
        eprintln!("[statusmaxxx] {} segment failed: {error:#}", segment.name());
        vec![Piece { text: format!("✗ {}", segment.name()), role: Role::Error, url: None }]
    })
}

pub fn ansi(segments: &[(Segment, Vec<Piece>)], config: &Config, hyperlinks: bool) -> String {
    segments
        .iter()
        .map(|(_, pieces)| pieces.iter().map(|piece| paint(piece, config.theme, hyperlinks)).collect::<String>())
        .collect::<Vec<_>>()
        .join(&ansi_separator(config))
}

pub fn ansi_separator(config: &Config) -> String {
    config.theme.paint(Role::Muted, &config.separator)
}

fn paint(piece: &Piece, theme: Theme, hyperlinks: bool) -> String {
    let painted = theme.paint(piece.role, &piece.text);
    match (&piece.url, hyperlinks) {
        (Some(url), true) => format!("\x1b]8;;{url}\x1b\\{painted}\x1b]8;;\x1b\\"),
        _ => painted,
    }
}

pub fn plain(segments: &[(Segment, Vec<Piece>)], separator: &str) -> String {
    segments
        .iter()
        .map(|(_, pieces)| pieces.iter().map(|piece| piece.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join(separator)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::config::Config;

    #[test]
    fn problems_show_in_the_line_instead_of_blanking_it() {
        let config = Config { segments: vec![Segment::Session], icons: BTreeSet::new(), ..Config::default() };
        let session = Session {
            cwd: PathBuf::from("/tmp"),
            model: None,
            context_used_percent: None,
            cost_usd: None,
            session_id: Some("82570de0-186d-4628-9996-a5b2a03955ea".into()),
            session_name: None,
        };
        let render = |problems: &[String]| render(Host::Amp, &config, &session, problems);
        assert!(render(&["config".to_string()]).contains("✗ config"));
        assert!(!render(&[]).contains('✗'));
    }
}
