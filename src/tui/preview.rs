use std::path::PathBuf;

use ansi_to_tui::IntoText;
use ratatui::text::{Line, Text};

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

/// What `host` would show for the sample session.
pub fn line(host: Host, config: &Config, sample: &Sample) -> Line<'static> {
    if host.tier() == Tier::BuiltIn {
        // Built-in agents draw their own items, so the preview names the item ids.
        let items: Vec<&str> =
            config.segments_for(host).iter().filter_map(|segment| host::builtin_item(host, *segment)).collect();
        return Line::from(card::muted(items.join(" · ")));
    }
    let session = as_sent_by(host, &sample.session);
    let sources = Sources::with_repo(&session, &config.icons, sample.repo.clone(), sample.issues.clone());
    let segments = render::segments(host, config, &sources);
    match host.output() {
        Output::Ansi { .. } | Output::Json { colored: true } => render::ansi(&segments, config, false)
            .into_text()
            .map(|text: Text| text.lines.into_iter().next().unwrap_or_default())
            .unwrap_or_else(|error| Line::from(card::focus(format!("Preview failed: {error}")))),
        // Agents that print escapes literally show plain text in their own colors.
        Output::Json { colored: false } => Line::from(card::text(render::plain(&segments, &config.separator))),
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
