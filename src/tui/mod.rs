//! `statusmaxxx config`: a screen-per-task card in the house TUI style. Every
//! change is saved as it is made, and each screen previews the line it affects.

mod card;
mod preview;
mod view;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::host::{Host, InstallState};
use crate::payload::Session;
use crate::segment::Segment;

pub fn run() -> Result<()> {
    let mut app = App::new(Config::load()?)?;
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    /// The shared list, or one agent's own list.
    Segments(Option<Host>),
    Agents,
    Agent(Host),
    Look,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeItem {
    Segments,
    Agents,
    Look,
    Quit,
}

const HOME: [HomeItem; 4] = [HomeItem::Segments, HomeItem::Agents, HomeItem::Look, HomeItem::Quit];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentItem {
    Install,
    Uninstall,
    Segments,
}

const AGENT_ITEMS: [AgentItem; 3] = [AgentItem::Install, AgentItem::Uninstall, AgentItem::Segments];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LookItem {
    Theme,
    Icons,
}

const LOOK: [LookItem; 2] = [LookItem::Theme, LookItem::Icons];

struct AgentRow {
    host: Host,
    detected: bool,
    state: InstallState,
}

struct App {
    config: Config,
    agents: Vec<AgentRow>,
    sample: Session,
    screen: Screen,
    cursor: usize,
    /// The agent Home and Look preview; ←/→ on Home cycles it.
    preview_host: Host,
    /// Outcome of the last action on this screen.
    notice: Vec<Line<'static>>,
    done: bool,
}

impl App {
    fn new(config: Config) -> Result<Self> {
        let mut app = Self {
            config,
            agents: Vec::new(),
            sample: preview::sample_session()?,
            screen: Screen::Home,
            cursor: 0,
            preview_host: Host::Claude,
            notice: Vec::new(),
            done: false,
        };
        app.reload_agents()?;
        app.preview_host = app.agents.iter().find(|row| row.detected).map_or(Host::Claude, |row| row.host);
        Ok(app)
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.done {
            terminal.draw(|frame| self.draw(frame))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle(key);
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = card::CARD_WIDTH.min(area.width);
        let (card, focus_line) = view::card(self, width as usize);
        let body_rows = (area.height as usize).saturating_sub(card.chrome_rows()).max(1);
        // Keep the focused item, and the description under it, in view.
        let scroll = (focus_line + 3).saturating_sub(body_rows).min(card.body.len().saturating_sub(body_rows));
        let lines = card.lines(body_rows, scroll);
        let height = (lines.len() as u16).min(area.height);
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;
        frame.render_widget(Paragraph::new(lines), Rect { x, y, width, height });
    }

    fn handle(&mut self, key: KeyEvent) {
        let control_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        if control_c || key.code == KeyCode::Char('q') {
            self.done = true;
            return;
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc => self.back(),
            KeyCode::Up | KeyCode::Char('k') if !shift => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') if !shift => self.move_cursor(1),
            KeyCode::Up | KeyCode::Char('K') => self.reorder(-1),
            KeyCode::Down | KeyCode::Char('J') => self.reorder(1),
            KeyCode::Left | KeyCode::Char('h') => self.step(-1),
            KeyCode::Right | KeyCode::Char('l') => self.step(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(),
            _ => {}
        }
    }

    fn item_count(&self) -> usize {
        match self.screen {
            Screen::Home => HOME.len(),
            Screen::Segments(_) => Segment::ALL.len(),
            Screen::Agents => self.agents.len(),
            Screen::Agent(_) => AGENT_ITEMS.len(),
            Screen::Look => LOOK.len(),
        }
    }

    fn move_cursor(&mut self, offset: isize) {
        let last = self.item_count() - 1;
        self.cursor = self.cursor.saturating_add_signed(offset).min(last);
    }

    fn open(&mut self, screen: Screen, cursor: usize) {
        self.screen = screen;
        self.cursor = cursor;
        self.notice.clear();
    }

    /// Back to the screen that opened this one, focused on the item that opened it.
    fn back(&mut self) {
        match self.screen {
            Screen::Home => self.done = true,
            Screen::Segments(None) => self.open(Screen::Home, 0),
            Screen::Agents => self.open(Screen::Home, 1),
            Screen::Look => self.open(Screen::Home, 2),
            Screen::Agent(host) => self.open(Screen::Agents, self.agent_index(host)),
            Screen::Segments(Some(host)) => self.open(Screen::Agent(host), 2),
        }
    }

    fn activate(&mut self) {
        match self.screen {
            Screen::Home => match HOME[self.cursor] {
                HomeItem::Segments => self.open(Screen::Segments(None), 0),
                HomeItem::Agents => self.open(Screen::Agents, 0),
                HomeItem::Look => self.open(Screen::Look, 0),
                HomeItem::Quit => self.done = true,
            },
            Screen::Segments(host) => self.toggle_segment(host),
            Screen::Agents => self.open(Screen::Agent(self.agents[self.cursor].host), 0),
            Screen::Agent(host) => match AGENT_ITEMS[self.cursor] {
                AgentItem::Install => self.apply(host, true),
                AgentItem::Uninstall => self.apply(host, false),
                AgentItem::Segments => {
                    if !self.config.hosts.contains_key(&host) {
                        self.config.segments_mut(Some(host));
                        self.save();
                    }
                    self.open(Screen::Segments(Some(host)), 0);
                }
            },
            Screen::Look => self.step(1),
        }
    }

    /// ←/→: the previewed agent on Home, or the choice on a chip row.
    fn step(&mut self, offset: isize) {
        match self.screen {
            Screen::Home => {
                let index = self.agent_index(self.preview_host) as isize + offset;
                self.preview_host = self.agents[index.rem_euclid(self.agents.len() as isize) as usize].host;
            }
            Screen::Agent(host) if AGENT_ITEMS[self.cursor] == AgentItem::Segments => {
                if self.config.hosts.remove(&host).is_none() {
                    self.config.segments_mut(Some(host));
                }
                self.save();
            }
            Screen::Look => {
                match LOOK[self.cursor] {
                    LookItem::Theme => {
                        self.config.theme =
                            if offset > 0 { self.config.theme.next() } else { self.config.theme.previous() }
                    }
                    LookItem::Icons => self.config.icons = !self.config.icons,
                }
                self.save();
            }
            _ => {}
        }
    }

    /// Segment rows: the shown ones in their order, then the hidden ones.
    fn segment_rows(&self, host: Option<Host>) -> Vec<(Segment, bool)> {
        let shown = self.shown_segments(host);
        let hidden = Segment::ALL.into_iter().filter(|segment| !shown.contains(segment));
        shown.iter().map(|segment| (*segment, true)).chain(hidden.map(|segment| (segment, false))).collect()
    }

    fn shown_segments(&self, host: Option<Host>) -> &[Segment] {
        match host {
            Some(host) => self.config.segments_for(host),
            None => &self.config.segments,
        }
    }

    fn toggle_segment(&mut self, host: Option<Host>) {
        let (segment, shown) = self.segment_rows(host)[self.cursor];
        let segments = self.config.segments_mut(host);
        if shown {
            segments.retain(|candidate| *candidate != segment);
        } else {
            segments.push(segment);
        }
        // The segment moved to the edge of the shown block; keep focus on it.
        self.cursor = self.segment_rows(host).iter().position(|(candidate, _)| *candidate == segment).unwrap_or(0);
        self.save();
    }

    /// Only shown segments have an order to change.
    fn reorder(&mut self, offset: isize) {
        let Screen::Segments(host) = self.screen else { return };
        let segments = self.config.segments_mut(host);
        let Some(target) = self.cursor.checked_add_signed(offset).filter(|target| *target < segments.len()) else {
            return;
        };
        if self.cursor >= segments.len() {
            return;
        }
        segments.swap(self.cursor, target);
        self.cursor = target;
        self.save();
    }

    fn save(&mut self) {
        if let Err(error) = self.config.save() {
            self.notice = vec![Line::from(card::focus(format!("Cannot save the config: {error:#}")))];
        }
    }

    fn apply(&mut self, host: Host, install: bool) {
        let outcome = if install { host.install(&self.config) } else { host.uninstall() };
        self.notice = match outcome {
            Ok(lines) if lines.is_empty() => vec![Line::from(card::muted("Nothing to change"))],
            Ok(lines) => lines.into_iter().map(|line| Line::from(card::text(line))).collect(),
            Err(error) => vec![Line::from(card::focus(format!("{error:#}")))],
        };
        if let Err(error) = self.reload_agents() {
            self.notice.push(Line::from(card::focus(format!("Cannot read agent state: {error:#}"))));
        }
    }

    fn reload_agents(&mut self) -> Result<()> {
        self.agents = Host::ALL
            .into_iter()
            .map(|host| {
                let detected = host.detected()?;
                let state = if detected { host.state(&self.config)? } else { InstallState::NotInstalled };
                Ok(AgentRow { host, detected, state })
            })
            .collect::<Result<_>>()?;
        Ok(())
    }

    fn agent_index(&self, host: Host) -> usize {
        self.agents.iter().position(|row| row.host == host).unwrap_or(0)
    }

    fn agent(&self, host: Host) -> &AgentRow {
        &self.agents[self.agent_index(host)]
    }
}
