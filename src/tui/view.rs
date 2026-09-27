//! Builds the card for the current screen. Returns the card and the body line
//! the focused item starts on, so the caller can keep it in view.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::card::{self, Card, chip, focus, muted, spread, text};
use super::{AGENT_ITEMS, AgentItem, App, HOME, HomeItem, LOOK, LookItem, Screen, preview};
use crate::host::{Host, InstallState, Tier};
use crate::segment::Segment;
use crate::theme::Theme;

pub fn card(app: &App, width: usize) -> (Card, usize) {
    let inner = Card::inner(width);
    let (body, focus_line, hint) = match app.screen {
        Screen::Home => home(app),
        Screen::Segments(host) => segments(app, host, inner),
        Screen::Agents => agents(app, inner),
        Screen::Agent(host) => agent(app, host, inner),
        Screen::Look => look(app, inner),
    };
    let mut body = body;
    if !app.notice.is_empty() {
        body.push(Line::default());
        body.extend(app.notice.iter().cloned());
    }
    (Card { width, header: header(app, inner), body, hint }, focus_line)
}

/// Centered: the previewed line, whose line it is, and what that agent leaves out.
fn header(app: &App, inner: usize) -> Vec<Line<'static>> {
    let host = previewed(app);
    let mut header = vec![
        card::center(card::fit(preview::line(host, &app.config, &app.sample), inner), inner),
        card::center(agent_control(host, cycles_preview(app.screen)), inner),
    ];
    let missing = preview::missing(host, &app.config);
    if !missing.is_empty() {
        header.push(card::center(Line::from(muted(why_missing(host, &missing))), inner));
    }
    header
}

/// `◂  Claude Code  ▸` where ←/→ switch the previewed agent, the bare name elsewhere.
/// The name sits in a slot as wide as the longest label, so the arrows never move.
fn agent_control(host: Host, cycles: bool) -> Line<'static> {
    if !cycles {
        return Line::from(text(host.label()));
    }
    let widest = Host::ALL.iter().map(|host| host.label().width()).max().unwrap_or(0);
    let mut spans = vec![muted("◂  ")];
    spans.extend(card::slot(text(host.label()), widest));
    spans.push(muted("  ▸"));
    Line::from(spans)
}

/// Screens where ←/→ belong to the preview rather than to a choice row.
pub fn cycles_preview(screen: Screen) -> bool {
    matches!(screen, Screen::Home | Screen::Segments(None))
}

fn previewed(app: &App) -> Host {
    match app.screen {
        Screen::Agents => app.agents[app.cursor].host,
        Screen::Agent(host) | Screen::Segments(Some(host)) => host,
        Screen::Home | Screen::Segments(None) | Screen::Look => app.preview_host,
    }
}

type Screenful = (Vec<Line<'static>>, usize, String);

fn home(app: &App) -> Screenful {
    let mut body = Vec::new();
    let mut focus_line = 0;
    for (index, item) in HOME.iter().enumerate() {
        if index > 0 {
            body.push(Line::default());
        }
        if index == app.cursor {
            focus_line = body.len();
        }
        let (name, description) = match item {
            HomeItem::Segments => ("Segments", segment_names(&app.config.segments)),
            HomeItem::Agents => ("Agents", agent_counts(app)),
            HomeItem::Look => ("Look", look_summary(app)),
            HomeItem::Quit => ("Quit", String::new()),
        };
        body.push(Line::from(name_span(name, index == app.cursor, true)));
        if !description.is_empty() {
            body.push(Line::from(muted(description)));
        }
    }
    let hint = match HOME[app.cursor] {
        HomeItem::Segments => "Choose what the line shows, and in what order",
        HomeItem::Agents => "Install into Claude Code, Amp, and the others you use",
        HomeItem::Look => "Theme and icons",
        HomeItem::Quit => "Changes are saved as you make them",
    };
    (body, focus_line, hint.to_string())
}

fn segments(app: &App, host: Option<Host>, inner: usize) -> Screenful {
    let scope = match host {
        Some(host) => format!("Only {} uses this list", host.label()),
        None => "Every agent uses this list, unless it has its own".to_string(),
    };
    let mut body = vec![Line::from(muted(scope)), Line::default()];
    let mut focus_line = 0;
    let rows = app.segment_rows(host);
    for (index, (segment, shown)) in rows.iter().enumerate() {
        if index > 0 {
            body.push(Line::default());
        }
        let focused = index == app.cursor;
        if focused {
            focus_line = body.len();
        }
        let state = if *shown { text("shown") } else { muted("hidden") };
        body.push(spread(vec![name_span(segment.name(), focused, *shown)], vec![state], inner));
        body.push(Line::from(match host {
            Some(host) if !host.supports(*segment) => focus(format!("{} cannot show this", host.label())),
            Some(_) => muted(segment.description()),
            None => muted(with_gaps(segment.description(), &agents_without(app, *segment))),
        }));
    }
    let hint = match rows[app.cursor] {
        (_, true) => "space hides it · J/K moves it · esc back",
        (_, false) => "space shows it · esc back",
    };
    (body, focus_line, hint.to_string())
}

fn agents(app: &App, inner: usize) -> Screenful {
    let body = app
        .agents
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let focused = index == app.cursor;
            let state = state_name(row.detected, &row.state);
            let state = if row.state == InstallState::Installed { text(state) } else { muted(state) };
            let own = if app.config.hosts.contains_key(&row.host) { muted("own segments  ") } else { Span::raw("") };
            spread(vec![name_span(row.host.label(), focused, row.detected)], vec![own, state], inner)
        })
        .collect();
    let hint = format!("enter opens {} · esc back", app.agents[app.cursor].host.label());
    (body, app.cursor, hint)
}

fn agent(app: &App, host: Host, inner: usize) -> Screenful {
    let mut about = host.note().to_string();
    if let InstallState::Occupied(other) = &app.agent(host).state {
        about.push_str(&format!(" It now shows <{other}>; Install replaces it, and Uninstall puts it back."));
    }
    let mut body: Vec<Line<'static>> = wrap(&about, inner).into_iter().map(|line| Line::from(muted(line))).collect();
    body.push(Line::default());
    let installed = app.agent(host).state == InstallState::Installed;
    let own = app.config.hosts.contains_key(&host);
    let mut focus_line = 0;
    for (index, item) in AGENT_ITEMS.iter().enumerate() {
        let focused = index == app.cursor;
        if index > 0 {
            body.push(Line::default());
        }
        if focused {
            focus_line = body.len();
        }
        match item {
            AgentItem::Install => {
                body.push(Line::from(name_span(if installed { "Reinstall" } else { "Install" }, focused, true)));
                body.push(Line::from(muted(install_summary(host))));
            }
            AgentItem::Uninstall => {
                body.push(Line::from(name_span("Uninstall", focused, true)));
                body.push(Line::from(muted("Removes what statusmaxxx wrote, and restores what it replaced")));
            }
            AgentItem::Segments => {
                let chips = vec![chip("Shared", !own, focused), muted("  "), chip("Own", own, focused)];
                body.push(spread(vec![name_span("Segments", focused, true)], chips, inner));
                body.push(Line::from(muted(if own {
                    segment_names(app.config.segments_for(host))
                } else {
                    "Same list as every other agent".to_string()
                })));
            }
        }
    }
    let hint = match AGENT_ITEMS[app.cursor] {
        AgentItem::Install | AgentItem::Uninstall => "enter runs it · esc back".to_string(),
        AgentItem::Segments => "←/→ shared or own · enter edits the list · esc back".to_string(),
    };
    (body, focus_line, hint)
}

fn look(app: &App, inner: usize) -> Screenful {
    let mut body = Vec::new();
    for (index, item) in LOOK.iter().enumerate() {
        let focused = index == app.cursor;
        if index > 0 {
            body.push(Line::default());
        }
        let (name, chips) = match item {
            LookItem::Theme => ("Theme", theme_picker(app.config.theme, focused)),
            LookItem::Icons => (
                "Icons",
                vec![chip("On", app.config.icons, focused), muted("  "), chip("Off", !app.config.icons, focused)],
            ),
        };
        body.push(spread(vec![name_span(name, focused, true)], chips, inner));
    }
    let hint = match LOOK[app.cursor] {
        LookItem::Theme => "←/→ picks a theme · esc back",
        LookItem::Icons => "←/→ switches Nerd Font icons · esc back",
    };
    (body, app.cursor * 2, hint.to_string())
}

/// `◂ short-giraffe ▸`: one picked value, since the full list outgrows narrow cards.
fn theme_picker(current: Theme, focused: bool) -> Vec<Span<'static>> {
    let position = Theme::ALL.iter().position(|theme| *theme == current).unwrap_or(0) + 1;
    let arrows = |arrow: &str| if focused { text(arrow) } else { muted(arrow) };
    let widest = Theme::ALL.iter().map(|theme| theme.name().width()).max().unwrap_or(0);
    let mut spans = vec![muted(format!("{position}/{}  ", Theme::ALL.len())), arrows("◂ ")];
    spans.extend(card::slot(focus(current.name()), widest));
    spans.push(arrows(" ▸"));
    spans
}

/// Focused names are red; inactive ones (hidden segment, missing agent) are muted.
fn name_span(name: &str, focused: bool, active: bool) -> Span<'static> {
    match (focused, active) {
        (true, _) => focus(name),
        (false, true) => text(name),
        (false, false) => muted(name),
    }
}

fn state_name(detected: bool, state: &InstallState) -> &'static str {
    match (detected, state) {
        (false, _) => "not found",
        (true, InstallState::Installed) => "installed",
        (true, InstallState::NotInstalled) => "available",
        (true, InstallState::Occupied(_)) => "another status line",
    }
}

fn install_summary(host: Host) -> String {
    match host.tier() {
        Tier::Command => "Sets the status line command and the session-start hook",
        Tier::Plugin => "Writes the plugin and the issue instruction",
        Tier::BuiltIn => "Picks the agent's own items that match your segments",
    }
    .to_string()
}

fn segment_names(segments: &[Segment]) -> String {
    segments.iter().map(|segment| segment.name()).collect::<Vec<_>>().join(" · ")
}

/// `Amp doesn't report its model and context`
fn why_missing(host: Host, missing: &[Segment]) -> String {
    let names: Vec<&str> = missing.iter().map(|segment| segment.name()).collect();
    let names = match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.join(""),
    };
    match host.tier() {
        Tier::BuiltIn => format!("{} has no item for {names}", host.label()),
        Tier::Command | Tier::Plugin => format!("{} doesn't report its {names}", host.label()),
    }
}

/// Detected agents that cannot show `segment`.
fn agents_without(app: &App, segment: Segment) -> Vec<Host> {
    app.agents.iter().filter(|row| row.detected && !row.host.supports(segment)).map(|row| row.host).collect()
}

/// `description · not in Amp, pi`, or a count once the list gets long.
fn with_gaps(description: &str, missing: &[Host]) -> String {
    match missing {
        [] => description.to_string(),
        [_, _, _, _, ..] => format!("{description} · not in {} of your agents", missing.len()),
        _ => {
            let names: Vec<&str> = missing.iter().map(|host| host.label()).collect();
            format!("{description} · not in {}", names.join(", "))
        }
    }
}

fn agent_counts(app: &App) -> String {
    let installed = app.agents.iter().filter(|row| row.detected && row.state == InstallState::Installed).count();
    let detected = app.agents.iter().filter(|row| row.detected).count();
    format!("{installed} installed · {} available · {} not found", detected - installed, app.agents.len() - detected)
}

fn look_summary(app: &App) -> String {
    let icons = if app.config.icons { "Nerd Font icons" } else { "no icons" };
    format!("{} theme · {icons}", app.config.theme.name())
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    lines
}
