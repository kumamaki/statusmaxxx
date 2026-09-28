//! Builds the card for the current screen. Returns the card and the body line
//! the focused item starts on, so the caller can keep it in view.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::card::{self, Card, chip, focus, muted, spread, text};
use super::{AGENT_ITEMS, AgentItem, App, HOME, HomeItem, STYLE_ITEMS, Screen, StyleItem, preview};
use crate::host::{Host, InstallState, Tier};
use crate::segment::Segment;
use crate::separator;
use crate::theme::Theme;

pub fn card(app: &App, width: usize) -> (Card, usize) {
    let inner = Card::inner(width);
    let (body, focus_line, hint) = match app.screen {
        Screen::Home => home(app),
        Screen::Segments(host) => segments(app, host, inner),
        Screen::Agents => agents(app, inner),
        Screen::Agent(host) => agent(app, host, inner),
        Screen::Style => style(app, inner),
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
    let marked = marked(app);
    let (line, marked_at) = preview::line(host, &app.config, &app.sample, marked);
    let line = card::fit(line, inner);
    let lead = inner.saturating_sub(line.width()) / 2;
    let mut header = Vec::new();
    // The row stays while a hidden segment is focused, so the header never jumps.
    if marked.is_some() {
        header.push(pointer(marked_at.filter(|column| *column < line.width()).map(|column| lead + column)));
    }
    header.extend([
        card::center(line, inner),
        Line::default(),
        card::center(agent_control(host, cycles_preview(app.screen)), inner),
    ]);
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
    screen == Screen::Home
}

/// Cells before a segment name: its icon when on, blank when off, so names never shift.
const ICON_SLOT: usize = 3;

fn icon_slot(app: &App, segment: Segment) -> Span<'static> {
    if app.config.icons.contains(&segment) {
        let icon = segment.icon();
        text(format!("{icon}{}", " ".repeat(ICON_SLOT.saturating_sub(icon.width()))))
    } else {
        Span::raw(" ".repeat(ICON_SLOT))
    }
}

/// `◂ shown ▸` on the focused row; the value keeps its column on every row.
fn shown_stepper(state: Span<'static>, focused: bool) -> Vec<Span<'static>> {
    let arrow = |glyph: &str| if focused { muted(glyph) } else { Span::raw(" ".repeat(glyph.width())) };
    let mut spans = vec![arrow("◂ ")];
    spans.extend(card::slot(state, "Hidden".width()));
    spans.push(arrow(" ▸"));
    spans
}

/// `↴` over the first cell of the focused segment; a blank row when it is not in the line.
fn pointer(column: Option<usize>) -> Line<'static> {
    match column {
        Some(column) => Line::from(vec![Span::raw(" ".repeat(column)), focus("↴")]),
        None => Line::default(),
    }
}

/// The focused segment row, pointed at in the preview so the row and its part of the line connect.
fn marked(app: &App) -> Option<Segment> {
    match app.screen {
        Screen::Segments(_) => Some(app.segment_order[app.cursor]),
        _ => None,
    }
}

fn previewed(app: &App) -> Host {
    match app.screen {
        Screen::Agents => app.agents[app.cursor].host,
        Screen::Agent(host) | Screen::Segments(Some(host)) => host,
        Screen::Home | Screen::Segments(None) | Screen::Style => app.preview_host,
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
            HomeItem::Style => {
                let separator = separator::name(&app.config.separator);
                ("Style", format!("{} theme · {separator} separator", app.config.theme.label()))
            }
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
        HomeItem::Style => "Colors of the status line, and what sits between segments",
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
        let state = match (focused && app.moving, *shown) {
            (true, _) => focus("Moving"),
            (false, true) => text("Shown"),
            (false, false) => muted("Hidden"),
        };
        let mut name = vec![icon_slot(app, *segment)];
        name.push(name_span(segment.label(), focused, *shown));
        body.push(spread(name, shown_stepper(state, focused && !app.moving), inner));
        let description = match host {
            Some(host) if !host.supports(*segment) => format!("{} cannot show this", host.label()),
            Some(_) => segment.description().to_string(),
            None => with_gaps(segment.description(), &agents_without(app, *segment)),
        };
        body.push(Line::from(vec![Span::raw(" ".repeat(ICON_SLOT)), muted(description)]));
    }
    let (segment, shown) = rows[app.cursor];
    let hint = match (app.moving, shown, segment.has_icon()) {
        (true, _, _) => "↑/↓ moves it · enter puts it down",
        (false, true, true) => "←/→ hides it · i toggles its icon · m moves it",
        (false, true, false) => "←/→ hides it · m moves it · esc back",
        (false, false, true) => "←/→ shows it · i toggles its icon · m moves it",
        (false, false, false) => "←/→ shows it · m moves it · esc back",
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
            let own = if app.config.hosts.contains_key(&row.host) { muted("Own segments  ") } else { Span::raw("") };
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

fn style(app: &App, inner: usize) -> Screenful {
    let themes: Vec<&str> = Theme::ALL.iter().map(|theme| theme.label()).collect();
    let separators: Vec<&str> = separator::PRESETS.iter().map(|(name, _)| *name).chain([separator::CUSTOM]).collect();
    // One slot width for both rows, so their arrows share columns.
    let widest = themes.iter().chain(&separators).map(|name| name.width()).max().unwrap_or(0);
    let mut body = Vec::new();
    for (index, item) in STYLE_ITEMS.iter().enumerate() {
        let focused = index == app.cursor;
        let (name, choice) = match item {
            StyleItem::Theme => {
                let current = app.config.theme;
                let position = Theme::ALL.iter().position(|theme| *theme == current);
                ("Theme", Choice { value: current.label(), position, count: Theme::ALL.len() })
            }
            StyleItem::Separator => {
                let current = &app.config.separator;
                let (value, position) = (separator::name(current), separator::position(current));
                ("Separator", Choice { value, position, count: separator::PRESETS.len() })
            }
        };
        body.push(spread(vec![name_span(name, focused, true)], picker(choice, widest, focused), inner));
    }
    (body, app.cursor, "↑/↓ picks a row · ←/→ changes it · esc back".to_string())
}

/// One value out of a list; `position` is none for a value the list lacks.
struct Choice {
    value: &'static str,
    position: Option<usize>,
    count: usize,
}

/// `3/7  ◂ short-giraffe ▸`: one picked value, since full lists outgrow narrow cards.
fn picker(choice: Choice, widest: usize, focused: bool) -> Vec<Span<'static>> {
    let arrows = |arrow: &str| if focused { text(arrow) } else { muted(arrow) };
    let position = choice.position.map_or("–".to_string(), |index| (index + 1).to_string());
    let value = if focused { focus(choice.value) } else { text(choice.value) };
    let mut spans = vec![muted(format!("{position}/{}  ", choice.count)), arrows("◂ ")];
    spans.extend(card::slot(value, widest));
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

fn state_name(detected: bool, state: &InstallState) -> String {
    match (detected, state) {
        (false, _) => "Not found".to_string(),
        (true, InstallState::Installed) => "Installed".to_string(),
        (true, InstallState::NotInstalled) => "Available".to_string(),
        (true, InstallState::Occupied(other)) => format!("Has its own status line ({})", short_command(other)),
    }
}

/// `statusline.sh` for `/Users/you/.factory/statusline.sh`: paths lose their folders, and
/// a long command keeps its start, so the name fits beside the agent.
fn short_command(command: &str) -> String {
    const LIMIT: usize = 24;
    let short = command
        .split_whitespace()
        .map(|word| word.rsplit('/').find(|part| !part.is_empty()).unwrap_or(word))
        .collect::<Vec<_>>()
        .join(" ");
    if short.chars().count() <= LIMIT {
        return short;
    }
    format!("{}…", short.chars().take(LIMIT - 1).collect::<String>())
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
    segments.iter().map(|segment| segment.label()).collect::<Vec<_>>().join(" · ")
}

/// `Amp doesn't report its model and context`
fn why_missing(host: Host, missing: &[Segment]) -> String {
    let names: Vec<String> = missing.iter().map(|segment| segment.label().to_lowercase()).collect();
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
