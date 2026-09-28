use std::path::PathBuf;

use ansi_to_tui::IntoText;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::card;
use crate::config::Config;
use crate::git::{Head, Repo};
use crate::host::{self, Host, Output, Tier};
use crate::issue::Issue;
use crate::payload::Session;
use crate::render;
use crate::segment::{Segment, Sources};

/// One made-up session for every agent, so previews differ only by the agent,
/// and every shown segment has something to show wherever the TUI runs.
pub struct Sample {
    session: Session,
    repo: Repo,
    issues: Vec<Issue>,
}

pub fn sample() -> Sample {
    let root = PathBuf::from("/home/you/src/auth");
    Sample {
        session: Session {
            cwd: root.join("web"),
            model: Some("Opus".into()),
            context_used_percent: Some(42.0),
            cost_usd: Some(1.23),
        },
        repo: Repo {
            git_dir: root.join(".git"),
            root,
            name: "shop".into(),
            worktree: Some("auth".into()),
            head: Head::Branch("eng-42".into()),
            changed_files: 3,
        },
        issues: vec![Issue {
            id: "ENG-42".into(),
            title: Some("Fix login".into()),
            state: Some("In Progress".into()),
            url: None,
        }],
    }
}

/// What `host` would show for the sample session, with `marked` underlined.
pub fn line(host: Host, config: &Config, sample: &Sample, marked: Option<Segment>) -> Line<'static> {
    let mark = |segment: Segment, spans: Vec<Span<'static>>| {
        if marked == Some(segment) { spans.into_iter().map(underlined).collect() } else { spans }
    };
    if host.tier() == Tier::BuiltIn {
        // Built-in agents draw their own items, so the preview names the item ids.
        let items = config.segments_for(host).iter().filter_map(|segment| {
            host::builtin_item(host, *segment).map(|item| mark(*segment, vec![card::muted(item)]))
        });
        return join(items, vec![card::muted(" · ")]);
    }
    let session = as_sent_by(host, &sample.session);
    let sources = Sources::with_repo(&session, &config.icons, sample.repo.clone(), sample.issues.clone());
    let colored = !matches!(host.output(), Output::Json { colored: false });
    // Drawn one segment at a time, so the marked one knows where it starts and ends.
    let draw = |ansi: String| match ansi.into_text() {
        Ok(text) => text.lines.into_iter().next().map_or_else(Vec::new, |line| line.spans),
        Err(error) => vec![card::focus(format!("Preview failed: {error}"))],
    };
    let segments = render::segments(host, config, &sources).into_iter().map(|part| {
        let spans = if colored {
            draw(render::ansi(std::slice::from_ref(&part), config, false))
        } else {
            // Agents that print escapes literally show plain text in their own colors.
            vec![card::text(render::plain(std::slice::from_ref(&part), ""))]
        };
        mark(part.0, spans)
    });
    let separator =
        if colored { draw(render::ansi_separator(config)) } else { vec![card::text(config.separator.clone())] };
    join(segments, separator)
}

fn underlined(span: Span<'static>) -> Span<'static> {
    let style = span.style.add_modifier(Modifier::UNDERLINED);
    span.style(style)
}

fn join(parts: impl Iterator<Item = Vec<Span<'static>>>, separator: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, part) in parts.enumerate() {
        if index > 0 {
            spans.extend(separator.iter().cloned());
        }
        spans.extend(part);
    }
    Line::from(spans)
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
