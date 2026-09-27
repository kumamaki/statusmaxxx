//! `statusmaxxx config`: pick segments, theme, and agents with a live preview
//! of what each agent will show.

use std::fs;

use ansi_to_tui::IntoText;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::host::{Host, InstallState, Output, Tier};
use crate::payload::{Payload, Session};
use crate::render;
use crate::segment::Segment;
use crate::{host, paths};

pub fn run() -> Result<()> {
    let mut app = App::new(Config::load()?)?;
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Segments,
    Agents,
}

struct AgentRow {
    host: Host,
    detected: bool,
    state: InstallState,
    preview: Line<'static>,
}

struct App {
    config: Config,
    saved: Config,
    pane: Pane,
    segment_cursor: ListState,
    agent_cursor: ListState,
    agents: Vec<AgentRow>,
    sample: Session,
    message: Line<'static>,
    quit_armed: bool,
    done: bool,
}

impl App {
    fn new(config: Config) -> Result<Self> {
        let mut app = Self {
            saved: config.clone(),
            config,
            pane: Pane::Segments,
            segment_cursor: ListState::default().with_selected(Some(0)),
            agent_cursor: ListState::default().with_selected(Some(0)),
            agents: Vec::new(),
            sample: sample_session()?,
            message: hint(),
            quit_armed: false,
            done: false,
        };
        app.reload_agents()?;
        Ok(app)
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.done {
            terminal.draw(|frame| self.draw(frame))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle(key)?;
            }
        }
        Ok(())
    }

    /// The agent whose override the segment list edits, if the selected agent has one.
    fn editing(&self) -> Option<Host> {
        let host = self.selected_agent();
        self.config.hosts.contains_key(&host).then_some(host)
    }

    fn selected_agent(&self) -> Host {
        self.selected_row().host
    }

    fn selected_row(&self) -> &AgentRow {
        &self.agents[self.agent_cursor.selected().unwrap_or(0)]
    }

    /// Enabled segments in their order, then the rest.
    fn segment_rows(&self) -> Vec<(Segment, bool)> {
        let enabled = match self.editing() {
            Some(host) => self.config.segments_for(host),
            None => &self.config.segments,
        };
        let disabled = Segment::ALL.into_iter().filter(|segment| !enabled.contains(segment));
        enabled.iter().map(|segment| (*segment, true)).chain(disabled.map(|segment| (segment, false))).collect()
    }

    fn handle(&mut self, key: KeyEvent) -> Result<()> {
        let quitting = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if !quitting {
            self.quit_armed = false;
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            _ if quitting => self.quit(),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                self.pane = if self.pane == Pane::Segments { Pane::Agents } else { Pane::Segments };
            }
            KeyCode::Char('K') if self.pane == Pane::Segments => self.move_segment(-1),
            KeyCode::Char('J') if self.pane == Pane::Segments => self.move_segment(1),
            KeyCode::Up if shift && self.pane == Pane::Segments => self.move_segment(-1),
            KeyCode::Down if shift && self.pane == Pane::Segments => self.move_segment(1),
            KeyCode::Up | KeyCode::Char('k') => self.cursor().select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.cursor().select_next(),
            KeyCode::Char(' ') | KeyCode::Enter if self.pane == Pane::Segments => self.toggle_segment(),
            KeyCode::Char('t') => {
                self.config.theme = self.config.theme.next();
                self.refresh_previews();
            }
            KeyCode::Char('n') => {
                self.config.icons = !self.config.icons;
                self.refresh_previews();
            }
            KeyCode::Char('o') => self.toggle_override(),
            KeyCode::Char('s') => self.save()?,
            KeyCode::Char('i') => self.apply(true),
            KeyCode::Char('u') => self.apply(false),
            _ => {}
        }
        self.clamp_cursors();
        Ok(())
    }

    fn cursor(&mut self) -> &mut ListState {
        match self.pane {
            Pane::Segments => &mut self.segment_cursor,
            Pane::Agents => &mut self.agent_cursor,
        }
    }

    fn clamp_cursors(&mut self) {
        let segments = self.segment_rows().len();
        let agents = self.agents.len();
        for (cursor, length) in [(&mut self.segment_cursor, segments), (&mut self.agent_cursor, agents)] {
            cursor.select(Some(cursor.selected().unwrap_or(0).min(length - 1)));
        }
    }

    fn quit(&mut self) {
        if self.config == self.saved || self.quit_armed {
            self.done = true;
        } else {
            self.quit_armed = true;
            self.message = Line::from("Unsaved changes: s saves, q again discards them").yellow();
        }
    }

    fn toggle_segment(&mut self) {
        let Some(index) = self.segment_cursor.selected() else { return };
        let (segment, enabled) = self.segment_rows()[index];
        let editing = self.editing();
        let segments = self.config.segments_mut(editing);
        if enabled {
            segments.retain(|candidate| *candidate != segment);
        } else {
            segments.push(segment);
            // Keep the cursor on the segment, which moved to the end of the enabled block.
            self.segment_cursor.select(Some(segments.len() - 1));
        }
        self.refresh_previews();
    }

    /// Only enabled segments have an order to change.
    fn move_segment(&mut self, offset: isize) {
        let Some(index) = self.segment_cursor.selected() else { return };
        let editing = self.editing();
        let segments = self.config.segments_mut(editing);
        let target = index.checked_add_signed(offset).filter(|target| *target < segments.len());
        let Some(target) = target.filter(|_| index < segments.len()) else { return };
        segments.swap(index, target);
        self.segment_cursor.select(Some(target));
        self.refresh_previews();
    }

    fn toggle_override(&mut self) {
        let host = self.selected_agent();
        if self.config.hosts.remove(&host).is_none() {
            self.config.segments_mut(Some(host));
            self.message = Line::from(format!("{} now has its own segments", host.label())).cyan();
        } else {
            self.message = Line::from(format!("{} uses the shared segments again", host.label())).cyan();
        }
        self.refresh_previews();
    }

    fn save(&mut self) -> Result<()> {
        self.config.save()?;
        self.saved = self.config.clone();
        self.message = Line::from(format!("Saved {}", paths::display(&paths::config_file()?))).green();
        Ok(())
    }

    /// Installing saves first: built-in agents are configured from the saved segments.
    fn apply(&mut self, install: bool) {
        let host = self.selected_agent();
        let outcome = (|| {
            if install {
                self.save()?;
                host.install(&self.config)
            } else {
                host.uninstall()
            }
        })();
        self.message = match outcome {
            Ok(lines) if lines.is_empty() => Line::from(format!("{}: nothing to change", host.label())).green(),
            Ok(lines) => Line::from(format!("{}: {}", host.label(), lines.join("; "))).green(),
            Err(error) => Line::from(format!("{}: {error:#}", host.label())).red(),
        };
        if let Err(error) = self.reload_agents() {
            self.message = Line::from(format!("Cannot read agent state: {error:#}")).red();
        }
    }

    fn reload_agents(&mut self) -> Result<()> {
        self.agents = Host::ALL
            .into_iter()
            .map(|host| {
                let detected = host.detected()?;
                let state = if detected { host.state(&self.config)? } else { InstallState::NotInstalled };
                Ok(AgentRow { host, detected, state, preview: Line::default() })
            })
            .collect::<Result<_>>()?;
        self.refresh_previews();
        Ok(())
    }

    fn refresh_previews(&mut self) {
        for row in &mut self.agents {
            row.preview = match last_session(row.host) {
                Ok(Some(session)) => preview(row.host, &self.config, &session),
                Ok(None) => preview(row.host, &self.config, &self.sample),
                Err(error) => Line::from(format!("last session unreadable: {error:#}")).red(),
            };
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [body, agents, footer] = Layout::vertical([
            Constraint::Length(Segment::ALL.len() as u16 + 2),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .areas(frame.area());
        let [segments, options] = Layout::horizontal([Constraint::Min(60), Constraint::Length(40)]).areas(body);
        self.draw_segments(frame, segments);
        self.draw_options(frame, options);
        self.draw_agents(frame, agents);
        let footer_lines = vec![self.message.clone(), self.agent_detail(), keys()];
        frame.render_widget(Paragraph::new(footer_lines).wrap(Wrap { trim: true }), footer);
    }

    fn draw_segments(&mut self, frame: &mut Frame, area: Rect) {
        let title = match self.editing() {
            Some(host) => format!(" Segments · {} only ", host.label()),
            None => " Segments · all agents ".to_string(),
        };
        let supported = |segment| self.editing().is_none_or(|host| host.supports(segment));
        let items: Vec<ListItem> = self
            .segment_rows()
            .into_iter()
            .map(|(segment, enabled)| {
                let check = if enabled { "[x] " } else { "[ ] " };
                let name = Span::styled(
                    format!("{:<10}", segment.name()),
                    if enabled { Style::new().bold() } else { Style::new() },
                );
                let mut description = Span::from(segment.description()).dark_gray();
                if !supported(segment) {
                    description = Span::from("not available in this agent").red();
                }
                ListItem::new(Line::from(vec![Span::from(check), name, description]))
            })
            .collect();
        let list = List::new(items).block(pane_block(&title, self.pane == Pane::Segments)).highlight_style(highlight());
        frame.render_stateful_widget(list, area, &mut self.segment_cursor);
    }

    fn draw_options(&self, frame: &mut Frame, area: Rect) {
        let row = |key: &str, label: &str, value: String| {
            Line::from(vec![
                Span::from(format!(" {key} ")).reversed(),
                Span::from(format!(" {label:<8}")),
                Span::from(value).bold(),
            ])
        };
        let lines = vec![
            row("t", "theme", self.config.theme.name().to_string()),
            row("n", "icons", if self.config.icons { "on (Nerd Font)".into() } else { "off".into() }),
            Line::default(),
            Line::from(" issue: statusmaxxx issue set <id>").dark_gray(),
        ];
        frame.render_widget(Paragraph::new(lines).block(pane_block(" Options ", false)), area);
    }

    fn draw_agents(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .agents
            .iter()
            .map(|row| {
                let (state, color) = match (&row.state, row.detected) {
                    (_, false) => ("not found".to_string(), Color::DarkGray),
                    (InstallState::Installed, true) => ("installed".to_string(), Color::Green),
                    (InstallState::NotInstalled, true) => ("available".to_string(), Color::Yellow),
                    (InstallState::Occupied(_), true) => ("other line".to_string(), Color::Magenta),
                };
                let overridden = if self.config.hosts.contains_key(&row.host) { "*" } else { " " };
                let mut spans = vec![
                    Span::from(format!("{:<14}", row.host.label())).bold(),
                    Span::from(overridden).cyan(),
                    Span::from(format!("{:<9}", tier_name(row.host.tier()))).dark_gray(),
                    Span::styled(format!("{state:<11}"), Style::new().fg(color)),
                ];
                spans.extend(row.preview.spans.iter().cloned());
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(pane_block(" Agents · preview (* own segments) ", self.pane == Pane::Agents))
            .highlight_style(highlight());
        frame.render_stateful_widget(list, area, &mut self.agent_cursor);
    }
}

impl App {
    /// What the selected agent's integration does, and anything it replaces.
    fn agent_detail(&self) -> Line<'static> {
        let row = self.selected_row();
        let mut text = format!("{}: {}", row.host.label(), row.host.note());
        if let InstallState::Occupied(other) = &row.state {
            text.push_str(&format!(" Now shows <{other}>; i replaces it and keeps a backup."));
        }
        Line::from(text).dark_gray()
    }
}

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Command => "command",
        Tier::Plugin => "plugin",
        Tier::BuiltIn => "built-in",
    }
}

/// Built-in agents draw their own items, so their preview lists item ids.
fn preview(host: Host, config: &Config, session: &Session) -> Line<'static> {
    if host.tier() == Tier::BuiltIn {
        let items: Vec<&str> =
            config.segments_for(host).iter().filter_map(|segment| host::builtin_item(host, *segment)).collect();
        return Line::from(items.join(" · ")).italic();
    }
    let segments = render::segments(host, config, session);
    match host.output() {
        Output::Ansi { .. } => render::ansi(&segments, config, false)
            .into_text()
            .map(|text: Text| text.lines.into_iter().next().unwrap_or_default())
            .unwrap_or_else(|error| Line::from(format!("preview failed: {error}")).red()),
        Output::Json => Line::from(render::plain(&segments, &config.separator)),
    }
}

/// The session this agent last sent, so the preview matches what it shows.
fn last_session(host: Host) -> Result<Option<Session>> {
    let path = paths::last_payload(host)?;
    match fs::read_to_string(&path) {
        Ok(contents) => Ok(Some(Payload::parse(&contents)?.into_session()?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Stands in for agents that have not rendered yet.
fn sample_session() -> Result<Session> {
    Ok(Session {
        cwd: std::env::current_dir()?,
        model: Some("Opus".into()),
        context_used_percent: Some(42.0),
        cost_usd: Some(1.23),
    })
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let block = Block::bordered().border_type(BorderType::Rounded).title(title.to_string());
    if focused { block.border_style(Style::new().cyan()) } else { block.border_style(Style::new().dark_gray()) }
}

fn highlight() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}

fn hint() -> Line<'static> {
    Line::from("Previews use each agent's last session, or this directory when it has not run yet.").dark_gray()
}

fn keys() -> Line<'static> {
    Line::from(
        "tab pane · j/k move · space toggle · J/K reorder · o own segments · i install · u uninstall · s save · q quit",
    )
    .dark_gray()
}
