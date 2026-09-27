use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::paths;
use crate::segment::Segment;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub segments: Vec<Segment>,
    pub theme: Theme,
    pub icons: bool,
    pub separator: String,
    /// Hosts that show a different segment list than `segments`.
    pub hosts: BTreeMap<Host, HostOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostOverride {
    pub segments: Vec<Segment>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            segments: vec![Segment::Worktree, Segment::Git, Segment::Issue, Segment::Model, Segment::Context],
            theme: Theme::default(),
            icons: true,
            separator: "  ".to_string(),
            hosts: BTreeMap::new(),
        }
    }
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
    fn host_overrides_replace_the_shared_segments() {
        let config: Config =
            toml::from_str("segments = [\"git\", \"model\"]\n[hosts.amp]\nsegments = [\"worktree\", \"issue\"]\n")
                .unwrap();
        assert_eq!(config.segments_for(Host::Claude), [Segment::Git, Segment::Model]);
        assert_eq!(config.segments_for(Host::Amp), [Segment::Worktree, Segment::Issue]);
    }
}
