//! Separators the TUI offers. The config stores the text itself, so any
//! string works; one that is not listed here shows as `custom`.

pub const PRESETS: [(&str, &str); 6] =
    [("space", "  "), ("dot", " · "), ("bar", " │ "), ("pipe", " | "), ("chevron", " › "), ("slash", " / ")];

pub const CUSTOM: &str = "custom";

pub fn position(text: &str) -> Option<usize> {
    PRESETS.iter().position(|(_, preset)| *preset == text)
}

pub fn name(text: &str) -> &'static str {
    position(text).map_or(CUSTOM, |index| PRESETS[index].0)
}

/// The preset `offset` steps from `current`, wrapping at the ends.
pub fn step(current: &str, offset: isize) -> &'static str {
    let count = PRESETS.len() as isize;
    // A custom separator sits just outside the list, so either direction lands on an end.
    let index = position(current).map_or(if offset > 0 { -1 } else { count }, |index| index as isize);
    PRESETS[(index + offset).rem_euclid(count) as usize].1
}
