use serde::{Deserialize, Serialize};

/// What a piece of text means; themes map roles to colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Path,
    Worktree,
    Branch,
    Dirty,
    Clean,
    Issue,
    Model,
    Context,
    ContextHigh,
    Cost,
    Muted,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    /// One of the 16 terminal palette colors, so the user's terminal theme applies.
    Palette(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Terminal,
    ShortGiraffe,
    Catppuccin,
    Dracula,
    Nord,
    Gruvbox,
    Light,
}

impl Theme {
    pub const ALL: [Theme; 7] = [
        Theme::Terminal,
        Theme::ShortGiraffe,
        Theme::Catppuccin,
        Theme::Dracula,
        Theme::Nord,
        Theme::Gruvbox,
        Theme::Light,
    ];

    /// How the TUI titles the theme; the config uses the kebab-case variant name.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Terminal => "Terminal",
            Theme::ShortGiraffe => "Short Giraffe",
            Theme::Catppuccin => "Catppuccin",
            Theme::Dracula => "Dracula",
            Theme::Nord => "Nord",
            Theme::Gruvbox => "Gruvbox",
            Theme::Light => "Light",
        }
    }

    pub fn next(self) -> Theme {
        self.offset(1)
    }

    pub fn previous(self) -> Theme {
        self.offset(Self::ALL.len() - 1)
    }

    fn offset(self, steps: usize) -> Theme {
        let index = Self::ALL.iter().position(|theme| *theme == self).unwrap_or(0);
        Self::ALL[(index + steps) % Self::ALL.len()]
    }

    /// Wraps `text` in the ANSI escape for `role`.
    pub fn paint(self, role: Role, text: &str) -> String {
        let code = match self.color(role) {
            Color::Palette(index) if index < 8 => format!("3{index}"),
            Color::Palette(index) => format!("9{}", index - 8),
            Color::Rgb(red, green, blue) => format!("38;2;{red};{green};{blue}"),
        };
        format!("\x1b[{code}m{text}\x1b[0m")
    }

    fn color(self, role: Role) -> Color {
        use Color::{Palette, Rgb};
        use Role::*;
        match self {
            Theme::Terminal => match role {
                Path => Palette(12),
                Worktree => Palette(13),
                Branch => Palette(11),
                Dirty => Palette(3),
                Clean => Palette(2),
                Issue => Palette(14),
                Model => Palette(5),
                Context => Palette(6),
                ContextHigh => Palette(1),
                Cost => Palette(2),
                Muted => Palette(8),
                Error => Palette(9),
            },
            // ~/Work/short-giraffe palette; the model takes the coral accent, as in its pi theme.
            Theme::ShortGiraffe => match role {
                Path => Rgb(0xBB, 0xD2, 0xEE),
                Worktree => Rgb(0xCF, 0xBA, 0xFA),
                Branch => Rgb(0xF5, 0xDA, 0x7A),
                Dirty => Rgb(0xFF, 0xB4, 0x80),
                Clean => Rgb(0xA6, 0xCC, 0x70),
                Issue => Rgb(0x5C, 0xCF, 0xE6),
                Model => Rgb(0xFF, 0xB6, 0x9E),
                Context => Rgb(0x45, 0xCA, 0xC2),
                ContextHigh => Rgb(0xF6, 0xAB, 0xA8),
                Cost => Rgb(0xA6, 0xCC, 0x70),
                Muted => Rgb(0x75, 0x81, 0xA0),
                Error => Rgb(0xF6, 0xAB, 0xA8),
            },
            Theme::Catppuccin => match role {
                Path => Rgb(137, 180, 250),
                Worktree => Rgb(203, 166, 247),
                Branch => Rgb(249, 226, 175),
                Dirty => Rgb(250, 179, 135),
                Clean => Rgb(166, 227, 161),
                Issue => Rgb(148, 226, 213),
                Model => Rgb(245, 194, 231),
                Context => Rgb(137, 220, 235),
                ContextHigh => Rgb(243, 139, 168),
                Cost => Rgb(166, 227, 161),
                Muted => Rgb(108, 112, 134),
                Error => Rgb(243, 139, 168),
            },
            Theme::Dracula => match role {
                Path => Rgb(189, 147, 249),
                Worktree => Rgb(255, 121, 198),
                Branch => Rgb(241, 250, 140),
                Dirty => Rgb(255, 184, 108),
                Clean => Rgb(80, 250, 123),
                Issue => Rgb(139, 233, 253),
                Model => Rgb(255, 121, 198),
                Context => Rgb(139, 233, 253),
                ContextHigh => Rgb(255, 85, 85),
                Cost => Rgb(80, 250, 123),
                Muted => Rgb(98, 114, 164),
                Error => Rgb(255, 85, 85),
            },
            Theme::Nord => match role {
                Path => Rgb(129, 161, 193),
                Worktree => Rgb(180, 142, 173),
                Branch => Rgb(235, 203, 139),
                Dirty => Rgb(208, 135, 112),
                Clean => Rgb(163, 190, 140),
                Issue => Rgb(136, 192, 208),
                Model => Rgb(180, 142, 173),
                Context => Rgb(143, 188, 187),
                ContextHigh => Rgb(191, 97, 106),
                Cost => Rgb(163, 190, 140),
                Muted => Rgb(76, 86, 106),
                Error => Rgb(191, 97, 106),
            },
            Theme::Gruvbox => match role {
                Path => Rgb(131, 165, 152),
                Worktree => Rgb(211, 134, 155),
                Branch => Rgb(250, 189, 47),
                Dirty => Rgb(254, 128, 25),
                Clean => Rgb(184, 187, 38),
                Issue => Rgb(142, 192, 124),
                Model => Rgb(211, 134, 155),
                Context => Rgb(142, 192, 124),
                ContextHigh => Rgb(251, 73, 52),
                Cost => Rgb(184, 187, 38),
                Muted => Rgb(146, 131, 116),
                Error => Rgb(251, 73, 52),
            },
            Theme::Light => match role {
                Path => Rgb(30, 102, 245),
                Worktree => Rgb(136, 57, 239),
                Branch => Rgb(223, 142, 29),
                Dirty => Rgb(254, 100, 11),
                Clean => Rgb(64, 160, 43),
                Issue => Rgb(23, 146, 153),
                Model => Rgb(234, 118, 203),
                Context => Rgb(4, 165, 229),
                ContextHigh => Rgb(210, 15, 57),
                Cost => Rgb(64, 160, 43),
                Muted => Rgb(140, 143, 161),
                Error => Rgb(210, 15, 57),
            },
        }
    }
}
