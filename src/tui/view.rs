//! Builds the card for the current screen. Returns the card and the body line
//! the focused item starts on, so the caller can keep it in view.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::card::{self, Card, focus, muted, spread, text};
use super::{AGENT_ITEMS, AgentItem, App, HOME, HomeItem, STYLE_ITEMS, Screen, StyleItem, preview};
use crate::host::{Host, InstallState, Tier};
use crate::segment::{IconFont, Segment};
use crate::separator;
use crate::theme::Theme;

pub fn card(app: &App, width: usize) -> (Card, usize) {
    let inner = Card::inner(width);
    let (body, focus_line, hint) = match app.screen {
        Screen::Home => home(app),
        Screen::Segments(scope) => segments(app, scope, inner),
        Screen::Agents => agents(app, inner),
        Screen::Agent(host) => agent(app, host, inner),
        Screen::Style => style(app, inner),
    };
    (Card { width, header: header(app, inner), body, notice: app.notice.clone(), hint }, focus_line)
}

/// Centered: the previewed line, the scope or agent it belongs to, and what that agent leaves out.
fn header(app: &App, inner: usize) -> Vec<Line<'static>> {
    let host = previewed(app);
    let list = previewed_list(app);
    let marked = marked(app);
    let (line, marked_at) = preview::line(host, list, &app.config, &app.sample, marked);
    let line = card::fit(line, inner);
    let lead = inner.saturating_sub(line.width()) / 2;
    let mut header = Vec::new();
    // The row stays while a hidden segment is focused, so the header never jumps.
    if marked.is_some() {
        header.push(pointer(marked_at.filter(|column| *column < line.width()).map(|column| lead + column)));
    }
    let control = match app.screen {
        Screen::Segments(scope) => scope_control(scope),
        _ => agent_control(host, cycles_preview(app.screen)),
    };
    header.extend([card::center(line, inner), Line::default(), card::center(control, inner)]);
    let missing = preview::missing(host, list);
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
    let widest = widest_agent_label();
    let mut spans = vec![muted("◂  ")];
    spans.extend(card::slot(text(host.label()), widest));
    spans.push(muted("  ▸"));
    Line::from(spans)
}

fn widest_agent_label() -> usize {
    Host::ALL.iter().map(|host| host.label().width()).max().unwrap_or(0)
}

/// `[tab]  All agents`: the Segments screen's scope. A keycap rather than ◂ ▸,
/// which stand for ←/→ — that pair already shows and hides segments.
fn scope_control(scope: Option<Host>) -> Line<'static> {
    const ALL_AGENTS: &str = "All agents";
    let label = scope.map_or(ALL_AGENTS, |host| host.label());
    let widest = widest_agent_label().max(ALL_AGENTS.width());
    let mut spans = vec![muted("[tab]  ")];
    spans.extend(card::slot(text(label), widest));
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
        let icon = segment.icon(app.config.icon_font);
        text(format!("{icon}{}", " ".repeat(ICON_SLOT.saturating_sub(icon.width()))))
    } else {
        Span::raw(" ".repeat(ICON_SLOT))
    }
}

/// `◂ shown ▸` on the focused row; the value keeps its column on every row.
fn shown_stepper(shown: bool, focused: bool, moving: bool) -> Vec<Span<'static>> {
    let arrow = |glyph: &str| if focused && !moving { text(glyph) } else { Span::raw(" ".repeat(glyph.width())) };
    let (state, active) = if moving {
        ("Moving", true)
    } else if shown {
        ("Shown", true)
    } else {
        ("Hidden", false)
    };
    let mut spans = vec![arrow("◂ ")];
    spans.extend(card::slot(name_span(state, focused, active), "Hidden".width()));
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

/// The segment list the preview draws: on Segments the list being edited, so
/// edits show up; anywhere else the list the previewed agent actually uses.
fn previewed_list(app: &App) -> &[Segment] {
    match app.screen {
        Screen::Segments(scope) => app.shown_segments(scope),
        _ => app.config.segments_for(previewed(app)),
    }
}

/// Labels of the agents with their own segment list, in config order.
fn agents_with_own_list(app: &App) -> Vec<&'static str> {
    app.config.hosts.keys().map(|host| host.label()).collect()
}

type Screenful = (Vec<Line<'static>>, usize, String);

/// On every screen that edits the config, which has no save step.
const SAVED: &str = "Saved as you go";

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
            HomeItem::Segments => ("Segments", shared_summary(app)),
            HomeItem::Agents => ("Agents", agent_counts(app)),
            HomeItem::Style => {
                let separator = separator::name(&app.config.separator);
                let icons = match app.config.icon_font {
                    IconFont::None => "no icons".to_string(),
                    font => format!("{} icons", font.label()),
                };
                ("Style", format!("{} theme · {separator} separator · {icons}", app.config.theme.label()))
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
        HomeItem::Style => "Colors and icons of the line, and what sits between segments",
        HomeItem::Quit => "Changes are saved as you make them",
    };
    (body, focus_line, hint.to_string())
}

fn segments(app: &App, scope: Option<Host>, inner: usize) -> Screenful {
    let own = scope.is_some_and(|host| app.config.hosts.contains_key(&host));
    // On the shared scope the preview still renders through one agent; name it.
    let ownership = match scope {
        Some(host) if own => format!("{} has its own list · r resets it to shared", host.label()),
        Some(host) => format!("{} uses the shared list · a change gives it its own", host.label()),
        None => match agents_with_own_list(app).as_slice() {
            [] => format!("Every agent uses this list · shown as {}", app.preview_host.label()),
            owners => {
                format!(
                    "Every other agent uses this list · own list: {} · shown as {}",
                    owners.join(", "),
                    app.preview_host.label()
                )
            }
        },
    };
    let mut body = vec![Line::from(muted(format!("{ownership} · {SAVED}"))), Line::default()];
    let mut focus_line = 0;
    let rows = app.segment_rows(scope);
    for (index, (segment, shown)) in rows.iter().enumerate() {
        if index > 0 {
            body.push(Line::default());
        }
        let focused = index == app.cursor;
        if focused {
            focus_line = body.len();
        }
        let mut name = vec![icon_slot(app, *segment)];
        name.push(name_span(segment.label(), focused, *shown));
        body.push(spread(name, shown_stepper(*shown, focused, focused && app.moving), inner));
        let description = match scope {
            Some(host) if !host.supports(*segment) => format!("{} cannot show this", host.label()),
            Some(_) => segment.description().to_string(),
            None => with_gaps(segment.description(), &agents_without(app, *segment)),
        };
        body.push(Line::from(vec![Span::raw(" ".repeat(ICON_SLOT)), muted(description)]));
    }
    let (segment, shown) = rows[app.cursor];
    // 80-column terminals clip the hint's tail, so the scope keys stay short.
    let keys = match (scope, own) {
        (Some(_), true) => "tab switches agent · r resets",
        (Some(_), false) => "tab switches agent",
        (None, _) => "tab picks an agent",
    };
    let icons = if scope.is_some() { "i toggles icon globally" } else { "i toggles its icon" };
    let hint = match (app.moving, shown, segment.has_icon()) {
        (true, _, _) => "↑/↓ moves it · enter puts it down".to_string(),
        (false, true, true) => format!("←/→ hides · {icons} · m moves · {keys}"),
        (false, true, false) => format!("←/→ hides · m moves · {keys}"),
        (false, false, true) => format!("←/→ shows · {icons} · m moves · {keys}"),
        (false, false, false) => format!("←/→ shows · m moves · {keys}"),
    };
    (body, focus_line, hint)
}

fn agents(app: &App, inner: usize) -> Screenful {
    let body = app
        .agents
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let focused = index == app.cursor;
            let state = state_name(row.detected, &row.state);
            let state = if row.state == InstallState::Installed { card::info(state) } else { muted(state) };
            let own = if app.config.hosts.contains_key(&row.host) { muted("Own list  ") } else { Span::raw("") };
            spread(vec![name_span(row.host.label(), focused, row.detected)], vec![own, state], inner)
        })
        .collect();
    let host = app.agents[app.cursor].host;
    let hint = if app.config.hosts.contains_key(&host) {
        format!("enter opens {} · tab on Segments edits its own list", host.label())
    } else {
        format!("enter opens {} · esc back", host.label())
    };
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
        }
    }
    (body, focus_line, "enter runs it · esc back".to_string())
}

fn style(app: &App, inner: usize) -> Screenful {
    let themes: Vec<&str> = Theme::ALL.iter().map(|theme| theme.label()).collect();
    let separators: Vec<&str> = separator::PRESETS.iter().map(|(name, _)| *name).chain([separator::CUSTOM]).collect();
    let fonts: Vec<&str> = IconFont::ALL.iter().map(|font| font.label()).collect();
    // One slot width for all rows, so their arrows share columns.
    let widest = themes.iter().chain(&separators).chain(&fonts).map(|name| name.width()).max().unwrap_or(0);
    let mut body = vec![Line::from(muted(SAVED)), Line::default()];
    let first_row = body.len();
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
            StyleItem::Icons => {
                let current = app.config.icon_font;
                let position = IconFont::ALL.iter().position(|font| *font == current);
                ("Icons", Choice { value: current.label(), position, count: IconFont::ALL.len() })
            }
        };
        body.push(spread(vec![name_span(name, focused, true)], picker(choice, widest, focused), inner));
    }
    (body, first_row + app.cursor, "↑/↓ picks a row · ←/→ changes it · esc back".to_string())
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

/// The shared list's names, plus which agents went their own way.
fn shared_summary(app: &App) -> String {
    let names = segment_names(&app.config.segments);
    match agents_with_own_list(app).as_slice() {
        [] => names,
        owners => format!("{names} · own list: {}", owners.join(", ")),
    }
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
