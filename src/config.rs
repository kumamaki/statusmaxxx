use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::paths;
use crate::segment::{IconFont, Segment};
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(deserialize_with = "Segment::deserialize_list")]
    pub segments: Vec<Segment>,
    pub theme: Theme,
    /// Segments drawn with an icon. `true`/`false` stand for all or none.
    #[serde(deserialize_with = "icons_from_list_or_bool")]
    pub icons: BTreeSet<Segment>,
    /// The glyph set `icons` draws from; `none` shows no icons at all.
    pub icon_font: IconFont,
    pub separator: String,
    /// Hosts that show a different segment list than `segments`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub hosts: BTreeMap<Host, HostOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostOverride {
    #[serde(deserialize_with = "Segment::deserialize_list")]
    pub segments: Vec<Segment>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            segments: vec![
                Segment::Directory,
                Segment::Worktree,
                Segment::Branch,
                Segment::Changes,
                Segment::Issue,
                Segment::Model,
                Segment::Context,
            ],
            theme: Theme::default(),
            icons: Segment::ALL.into_iter().filter(|segment| segment.has_icon()).collect(),
            icon_font: IconFont::default(),
            separator: " · ".to_string(),
            hosts: BTreeMap::new(),
        }
    }
}

fn icons_from_list_or_bool<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BTreeSet<Segment>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Icons {
        All(bool),
        Some(Vec<String>),
    }
    Ok(match Icons::deserialize(deserializer)? {
        Icons::All(true) => Segment::ALL.into_iter().filter(|segment| segment.has_icon()).collect(),
        Icons::All(false) => BTreeSet::new(),
        Icons::Some(names) => Segment::parse_names(&names).map_err(serde::de::Error::custom)?.into_iter().collect(),
    })
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = paths::config_file()?;
        match fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).with_context(|| format!("Invalid config <{}>", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        paths::write_file(&paths::config_file()?, &toml::to_string_pretty(self)?)
    }

    pub fn segments_for(&self, host: Host) -> &[Segment] {
        self.hosts.get(&host).map_or(&self.segments, |host| &host.segments)
    }

    /// The list a host override edits, created from the shared list on first edit.
    pub fn segments_mut(&mut self, host: Option<Host>) -> &mut Vec<Segment> {
        match host {
            Some(host) => {
                let shared = self.segments.clone();
                &mut self.hosts.entry(host).or_insert(HostOverride { segments: shared }).segments
            }
            None => &mut self.segments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_accept_a_list_or_all_or_none() {
        let icons = |toml: &str| toml::from_str::<Config>(toml).unwrap().icons.into_iter().collect::<Vec<_>>();
        assert_eq!(icons("icons = [\"branch\", \"model\"]"), [Segment::Branch, Segment::Model]);
        assert_eq!(icons("icons = false"), []);
        assert!(icons("icons = true").contains(&Segment::Worktree) && !icons("icons = true").contains(&Segment::Cost));
    }

    #[test]
    fn icon_font_defaults_to_nerd_and_reads_the_other_picks() {
        assert_eq!(toml::from_str::<Config>("").unwrap().icon_font, IconFont::Nerd);
        assert_eq!(toml::from_str::<Config>(r#"icon_font = "unicode""#).unwrap().icon_font, IconFont::Unicode);
        assert_eq!(toml::from_str::<Config>(r#"icon_font = "none""#).unwrap().icon_font, IconFont::None);
    }

    #[test]
    fn host_overrides_replace_the_shared_segments() {
        let config: Config =
            toml::from_str("segments = [\"git\", \"model\"]\n[hosts.amp]\nsegments = [\"worktree\", \"issue\"]\n")
                .unwrap();
        // `git` names branch and changes together.
        assert_eq!(config.segments_for(Host::Claude), [Segment::Branch, Segment::Changes, Segment::Model]);
        assert_eq!(config.segments_for(Host::Amp), [Segment::Worktree, Segment::Issue]);
    }
}
