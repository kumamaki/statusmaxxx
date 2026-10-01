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
use crate::segment::Segment;
use crate::separator;

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
    /// The shared list, or one agent's scope (its own list once it edits).
    Segments(Option<Host>),
    Agents,
    Agent(Host),
    Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeItem {
    Segments,
    Agents,
    Style,
    Quit,
}

const HOME: [HomeItem; 4] = [HomeItem::Segments, HomeItem::Agents, HomeItem::Style, HomeItem::Quit];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentItem {
    Install,
    Uninstall,
}

const AGENT_ITEMS: [AgentItem; 2] = [AgentItem::Install, AgentItem::Uninstall];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleItem {
    Theme,
    Separator,
}

const STYLE_ITEMS: [StyleItem; 2] = [StyleItem::Theme, StyleItem::Separator];

struct AgentRow {
    host: Host,
    detected: bool,
    state: InstallState,
}

struct App {
    config: Config,
    /// The config as last written, to tell built-in agents what changed.
    saved: Config,
    agents: Vec<AgentRow>,
    sample: preview::Sample,
    screen: Screen,
    cursor: usize,
    /// The agent the shared screens preview through; ←/→ on Home cycles it.
    preview_host: Host,
    /// Outcome of the last action on this screen.
    notice: Vec<Line<'static>>,
    /// The Segments screen's rows, fixed for the visit so toggling never moves one.
    segment_order: Vec<Segment>,
    /// The focused segment is picked up; ↑/↓ carry it.
    moving: bool,
    done: bool,
}

impl App {
    fn new(config: Config) -> Result<Self> {
        let mut app = Self {
            saved: config.clone(),
            config,
            agents: Vec::new(),
            sample: preview::sample(),
            screen: Screen::Home,
            cursor: 0,
            preview_host: Host::Claude,
            notice: Vec::new(),
            segment_order: Vec::new(),
            moving: false,
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
        if self.moving {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.carry(-1),
                KeyCode::Down | KeyCode::Char('j') => self.carry(1),
                KeyCode::Enter | KeyCode::Char(' ' | 'm') | KeyCode::Esc => self.moving = false,
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.back(),
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Tab => self.switch_scope(1),
            KeyCode::BackTab => self.switch_scope(-1),
            KeyCode::Char('m') if matches!(self.screen, Screen::Segments(_)) => self.moving = true,
            KeyCode::Char('i') if matches!(self.screen, Screen::Segments(_)) => self.toggle_icon(),
            KeyCode::Char('r') if matches!(self.screen, Screen::Segments(Some(_))) => self.reset_list(),
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
            Screen::Style => STYLE_ITEMS.len(),
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
        self.moving = false;
        if let Screen::Segments(scope) = screen {
            let shown = self.shown_segments(scope);
            let hidden = Segment::ALL.into_iter().filter(|segment| !shown.contains(segment));
            self.segment_order = shown.iter().copied().chain(hidden).collect();
        }
    }

    /// Back to the screen that opened this one, focused on the item that opened it.
    fn back(&mut self) {
        match self.screen {
            Screen::Home => self.done = true,
            Screen::Segments(_) => self.open(Screen::Home, 0),
            Screen::Agents => self.open(Screen::Home, 1),
            Screen::Style => self.open(Screen::Home, 2),
            Screen::Agent(host) => self.open(Screen::Agents, self.agent_index(host)),
        }
    }

    fn activate(&mut self) {
        match self.screen {
            Screen::Home => match HOME[self.cursor] {
                HomeItem::Segments => self.open(Screen::Segments(None), 0),
                HomeItem::Agents => self.open(Screen::Agents, 0),
                HomeItem::Style => self.open(Screen::Style, 0),
                HomeItem::Quit => self.done = true,
            },
            Screen::Segments(scope) => self.toggle_segment(scope),
            Screen::Agents => self.open(Screen::Agent(self.agents[self.cursor].host), 0),
            Screen::Agent(host) => match AGENT_ITEMS[self.cursor] {
                AgentItem::Install => self.apply(host, true),
                AgentItem::Uninstall => self.apply(host, false),
            },
            Screen::Style => self.step(1),
        }
    }

    /// ←/→: the previewed agent where the header offers it, or the choice on a row.
    fn step(&mut self, offset: isize) {
        match self.screen {
            screen if view::cycles_preview(screen) => {
                let choices = self.detected_or_all();
                let index = choices.iter().position(|host| *host == self.preview_host).unwrap_or(0) as isize + offset;
                self.preview_host = choices[index.rem_euclid(choices.len() as isize) as usize];
            }
            Screen::Segments(scope) => self.toggle_segment(scope),
            Screen::Style => {
                match STYLE_ITEMS[self.cursor] {
                    StyleItem::Theme => {
                        self.config.theme =
                            if offset > 0 { self.config.theme.next() } else { self.config.theme.previous() };
                    }
                    StyleItem::Separator => {
                        self.config.separator = separator::step(&self.config.separator, offset).to_string();
                    }
                }
                self.save();
            }
            _ => {}
        }
    }

    /// `tab` on Segments cycles the scopes `scope_choices` lists.
    fn switch_scope(&mut self, offset: isize) {
        let Screen::Segments(current) = self.screen else { return };
        let owned: Vec<Host> = self.config.hosts.keys().copied().collect();
        let scopes = scope_choices(&self.detected_or_all(), &owned);
        let index = scopes.iter().position(|scope| *scope == current).unwrap_or(0) as isize + offset;
        self.open_segments(scopes[index.rem_euclid(scopes.len() as isize) as usize]);
    }

    /// `r` on an agent's own list: back to following the shared one.
    fn reset_list(&mut self) {
        let Screen::Segments(Some(host)) = self.screen else { return };
        if self.config.hosts.remove(&host).is_none() {
            return;
        }
        self.open_segments(Some(host));
        self.save();
        self.notice.insert(0, Line::from(card::success(format!("✓ {} follows the shared list", host.label()))));
    }

    /// The agents found on this machine; all of them when none are.
    fn detected_or_all(&self) -> Vec<Host> {
        let mut choices: Vec<Host> = self.agents.iter().filter(|row| row.detected).map(|row| row.host).collect();
        if choices.is_empty() {
            choices = Host::ALL.to_vec();
        }
        choices
    }

    /// Opens the Segments screen at `scope` with the cursor on the segment it was on.
    fn open_segments(&mut self, scope: Option<Host>) {
        let segment = self.segment_order.get(self.cursor).copied();
        self.open(Screen::Segments(scope), 0);
        if let Some(index) = segment.and_then(|segment| self.segment_order.iter().position(|row| *row == segment)) {
            self.cursor = index;
        }
    }

    /// Every segment in this visit's fixed order, with whether it is shown.
    fn segment_rows(&self, scope: Option<Host>) -> Vec<(Segment, bool)> {
        let shown = self.shown_segments(scope);
        self.segment_order.iter().map(|segment| (*segment, shown.contains(segment))).collect()
    }

    fn shown_segments(&self, scope: Option<Host>) -> &[Segment] {
        match scope {
            Some(host) => self.config.segments_for(host),
            None => &self.config.segments,
        }
    }

    fn toggle_segment(&mut self, scope: Option<Host>) {
        let (segment, shown) = self.segment_rows(scope)[self.cursor];
        self.write_shown(scope, |candidate, is_shown| if candidate == segment { !shown } else { is_shown });
    }

    /// Icons are shared by every agent, like the theme.
    fn toggle_icon(&mut self) {
        let segment = self.segment_order[self.cursor];
        if !segment.has_icon() {
            return;
        }
        if !self.config.icons.remove(&segment) {
            self.config.icons.insert(segment);
        }
        self.save();
    }

    /// Moves the picked-up segment one row; a shown one moves in the status line too.
    fn carry(&mut self, offset: isize) {
        let Screen::Segments(scope) = self.screen else { return };
        let Some(target) = self.cursor.checked_add_signed(offset).filter(|target| *target < self.segment_order.len())
        else {
            return;
        };
        self.segment_order.swap(self.cursor, target);
        self.cursor = target;
        self.write_shown(scope, |_, is_shown| is_shown);
    }

    /// The status line shows the segments `keep` picks, in the screen's order.
    /// Looking at an agent's list, or moving a hidden row, creates no override;
    /// the first change that alters the shown list does.
    fn write_shown(&mut self, scope: Option<Host>, keep: impl Fn(Segment, bool) -> bool) {
        let shown: Vec<Segment> = self
            .segment_rows(scope)
            .into_iter()
            .filter(|(segment, shown)| keep(*segment, *shown))
            .map(|(segment, _)| segment)
            .collect();
        if self.shown_segments(scope) == shown.as_slice() {
            return;
        }
        *self.config.segments_mut(scope) = shown;
        self.save();
    }

    /// Writes the config, and the item lists of built-in agents that copy it.
    fn save(&mut self) {
        if let Err(error) = self.config.save() {
            self.notice = vec![Line::from(card::focus(format!("Cannot save the config: {error:#}")))];
            return;
        }
        self.notice = Host::ALL.into_iter().filter_map(|host| self.sync(host)).collect();
        self.saved = self.config.clone();
        if let Err(error) = self.reload_agents() {
            self.notice.push(Line::from(card::focus(format!("Cannot read agent state: {error:#}"))));
        }
    }

    /// `✓ Updated Codex CLI`, when `host` needed rewriting.
    fn sync(&self, host: Host) -> Option<Line<'static>> {
        match host.sync(&self.saved, &self.config) {
            Ok(None) => None,
            Ok(Some(items)) => {
                let tail =
                    if items.is_empty() { " · it shows no items now" } else { " · new sessions show the change" };
                Some(Line::from(vec![card::success(format!("✓ Updated {}", host.label())), card::muted(tail)]))
            }
            Err(error) => Some(Line::from(card::focus(format!("Cannot update {}: {error:#}", host.label())))),
        }
    }

    fn apply(&mut self, host: Host, install: bool) {
        let outcome = if install { host.install(&self.config) } else { host.uninstall(&self.config) };
        self.notice = match outcome {
            Ok(lines) if lines.is_empty() => vec![Line::from(card::muted("Nothing to change"))],
            Ok(lines) => {
                let (done, tail) = if install {
                    (format!("✓ Installed in {}", host.label()), " · new sessions show the line")
                } else {
                    (format!("✓ Removed from {}", host.label()), "")
                };
                let headline = Line::from(vec![card::success(done), card::muted(tail)]);
                // Indented under the headline's text, past the check mark.
                let details = lines.into_iter().map(|line| Line::from(card::muted(format!("  {line}"))));
                std::iter::once(headline).chain(details).collect()
            }
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

/// `tab`'s stops on Segments: All agents, the detected ones, then agents whose
/// own list survives even after their install is gone.
fn scope_choices(detected: &[Host], owned: &[Host]) -> Vec<Option<Host>> {
    let reachable = detected.iter().copied().chain(owned.iter().copied().filter(|host| !detected.contains(host)));
    std::iter::once(None).chain(reachable.map(Some)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_stay_reachable_for_an_agent_whose_install_is_gone() {
        let scopes = scope_choices(&[Host::Claude], &[Host::Claude, Host::Gemini]);
        assert_eq!(scopes, vec![None, Some(Host::Claude), Some(Host::Gemini)]);
    }
}
