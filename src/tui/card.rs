//! The hug-tab card shared with the other house TUIs (fam, Musik): a centered
//! card with the app name and `esc` as tabs, a header, a body, and a one-line
//! hint. Widths are terminal cells.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

pub const CARD_WIDTH: u16 = 100;
const PAD: usize = 3;
const TAB_LEFT: &str = "statusmaxxx";
const TAB_RIGHT: &str = "esc";
const TAB_LEFT_INSET: usize = 3;
const TAB_RIGHT_INSET: usize = 2;

pub const TEXT: Color = Color::Rgb(0xc8, 0xc8, 0xc8);
pub const MUTED: Color = Color::Rgb(0x7a, 0x7a, 0x7a);
pub const FOCUS: Color = Color::Rgb(0xff, 0x45, 0x3a);
/// Status only: an action that finished.
pub const SUCCESS: Color = Color::Rgb(0x30, 0xd1, 0x58);
const BORDER: Color = Color::Rgb(0x5a, 0x5a, 0x5a);

pub fn text(content: impl Into<String>) -> Span<'static> {
    Span::styled(content.into(), Style::new().fg(TEXT))
}

pub fn muted(content: impl Into<String>) -> Span<'static> {
    Span::styled(content.into(), Style::new().fg(MUTED))
}

pub fn focus(content: impl Into<String>) -> Span<'static> {
    Span::styled(content.into(), Style::new().fg(FOCUS))
}

pub fn success(content: impl Into<String>) -> Span<'static> {
    Span::styled(content.into(), Style::new().fg(SUCCESS))
}

/// One option of a choice row; the picked one is red, and bracketed while its row has focus.
pub fn chip(name: &str, picked: bool, row_focused: bool) -> Span<'static> {
    match (picked, row_focused) {
        (true, true) => focus(format!("[{name}]")),
        (true, false) => focus(name),
        (false, _) => muted(name),
    }
}

pub struct Card {
    pub width: usize,
    pub header: Vec<Line<'static>>,
    pub body: Vec<Line<'static>>,
    pub hint: String,
}

impl Card {
    /// The body's usable width inside the borders and padding.
    pub fn inner(width: usize) -> usize {
        width.saturating_sub(2 + PAD * 2)
    }

    /// Lines of the whole card. `body_rows` caps the body so the card fits the
    /// terminal; the window starts at `scroll`.
    pub fn lines(&self, body_rows: usize, scroll: usize) -> Vec<Line<'static>> {
        let inner = Self::inner(self.width);
        let mut lines = vec![self.tab_top(), self.tab_join(), self.row(Line::default(), inner)];
        lines.extend(self.header.iter().map(|line| self.row(line.clone(), inner)));
        lines.push(self.row(Line::default(), inner));
        lines.push(self.rule('├', '┤'));
        lines.push(self.row(Line::default(), inner));
        lines.extend(self.body.iter().skip(scroll).take(body_rows).map(|line| self.row(line.clone(), inner)));
        lines.push(self.row(Line::default(), inner));
        lines.push(self.rule('├', '┤'));
        lines.push(self.hint_row());
        lines.push(self.rule('╰', '╯'));
        lines
    }

    /// Rows the card spends on everything but the body.
    pub fn chrome_rows(&self) -> usize {
        self.header.len() + 11
    }

    fn tab_width(label: &str, inset: usize) -> usize {
        2 + inset * 2 + label.chars().count()
    }

    fn tab_middle(&self) -> usize {
        self.width
            .saturating_sub(Self::tab_width(TAB_LEFT, TAB_LEFT_INSET) + Self::tab_width(TAB_RIGHT, TAB_RIGHT_INSET))
    }

    fn tab_top(&self) -> Line<'static> {
        let tab = |label: &str, inset: usize| format!("╭{}╮", "─".repeat(Self::tab_width(label, inset) - 2));
        border(format!(
            "{}{}{}",
            tab(TAB_LEFT, TAB_LEFT_INSET),
            " ".repeat(self.tab_middle()),
            tab(TAB_RIGHT, TAB_RIGHT_INSET)
        ))
    }

    fn tab_join(&self) -> Line<'static> {
        let left_pad = " ".repeat(TAB_LEFT_INSET);
        let right_pad = " ".repeat(TAB_RIGHT_INSET);
        Line::from(vec![
            border_span(format!("│{left_pad}")),
            text(TAB_LEFT),
            border_span(format!("{left_pad}╰{}╯{right_pad}", "─".repeat(self.tab_middle()))),
            muted(TAB_RIGHT),
            border_span(format!("{right_pad}│")),
        ])
    }

    fn rule(&self, left: char, right: char) -> Line<'static> {
        border(format!("{left}{}{right}", "─".repeat(self.width.saturating_sub(2))))
    }

    /// `│   content…   │`, clipped or padded to the inner width.
    fn row(&self, content: Line<'static>, inner: usize) -> Line<'static> {
        let content = fit(content, inner);
        let fill = inner.saturating_sub(content.width());
        let pad = " ".repeat(PAD);
        let mut spans = vec![border_span("│"), Span::raw(pad.clone())];
        spans.extend(content.spans);
        spans.push(Span::raw(" ".repeat(fill)));
        spans.push(Span::raw(pad));
        spans.push(border_span("│"));
        Line::from(spans)
    }

    fn hint_row(&self) -> Line<'static> {
        let inner = self.width.saturating_sub(2);
        let hint: String = self.hint.chars().take(inner.saturating_sub(1)).collect();
        let lead = inner.saturating_sub(1 + hint.chars().count());
        Line::from(vec![border_span("│"), Span::raw(" ".repeat(lead)), muted(hint), Span::raw(" "), border_span("│")])
    }
}

/// `left` then `right` pushed to the far edge of `width`.
pub fn spread(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let used = Line::from(left.clone()).width() + Line::from(right.clone()).width();
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used).max(1))));
    spans.extend(right);
    Line::from(spans)
}

/// `content` centered in a slot of `width` cells, for controls whose edges must not move.
pub fn slot(content: Span<'static>, width: usize) -> Vec<Span<'static>> {
    let spare = width.saturating_sub(content.width());
    vec![Span::raw(" ".repeat(spare / 2)), content, Span::raw(" ".repeat(spare - spare / 2))]
}

/// Pads `line` on the left so it sits in the middle of `width`.
pub fn center(line: Line<'static>, width: usize) -> Line<'static> {
    let lead = width.saturating_sub(line.width()) / 2;
    let mut spans = vec![Span::raw(" ".repeat(lead))];
    spans.extend(line.spans);
    Line::from(spans)
}

/// Clips a styled line to `width` cells, ending in `…` when anything was cut.
pub fn fit(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let mut left = width.saturating_sub(1);
    let mut spans = Vec::new();
    for span in line.spans {
        if left == 0 {
            break;
        }
        let kept: String = span
            .content
            .chars()
            .scan(0, |used, character| {
                *used += character.width().unwrap_or(0);
                (*used <= left).then_some(character)
            })
            .collect();
        left -= kept.chars().filter_map(UnicodeWidthChar::width).sum::<usize>();
        spans.push(Span::styled(kept, span.style));
    }
    spans.push(muted("…"));
    Line::from(spans)
}

fn border_span(content: impl Into<String>) -> Span<'static> {
    Span::styled(content.into(), Style::new().fg(BORDER))
}

fn border(content: String) -> Line<'static> {
    Line::from(border_span(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_line_is_exactly_the_card_width() {
        for width in [CARD_WIDTH as usize, 40] {
            let card = Card {
                width,
                header: vec![Line::from("a header wider than any card could ever hold ".repeat(4))],
                body: vec![spread(vec![text("left")], vec![muted("right")], Card::inner(width))],
                hint: "hint".repeat(40),
            };
            for (index, line) in card.lines(10, 0).iter().enumerate() {
                assert_eq!(line.width(), width, "line <{index}> at card width <{width}>");
            }
        }
    }
}
