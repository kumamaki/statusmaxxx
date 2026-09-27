//! Scripts agents run instead of `statusmaxxx …` command lines: some agents
//! spawn commands without a shell, where arguments in the string break.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{Host, shell_quote};
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrapper {
    StatusLine,
    SessionStart,
}

impl Wrapper {
    fn file_name(self, host: Host) -> String {
        match self {
            Wrapper::StatusLine => host.id().to_string(),
            Wrapper::SessionStart => format!("{}-session-start", host.id()),
        }
    }

    fn arguments(self, host: Host) -> String {
        match self {
            Wrapper::StatusLine => format!("render --host {host}"),
            Wrapper::SessionStart => format!("hook session-start --host {host}"),
        }
    }

    pub fn path(self, host: Host) -> Result<PathBuf> {
        Ok(paths::config_dir()?.join("hosts").join(self.file_name(host)))
    }

    /// The path as agents store it in their settings.
    pub fn command(self, host: Host) -> Result<String> {
        Ok(self.path(host)?.to_string_lossy().into_owned())
    }

    pub fn write(self, host: Host) -> Result<String> {
        let path = self.path(host)?;
        let script = format!(
            "#!/bin/sh\n# Managed by statusmaxxx; `statusmaxxx uninstall {host}` removes it.\nexec {} {}\n",
            shell_quote(&paths::binary()?.to_string_lossy()),
            self.arguments(host),
        );
        paths::write_file(&path, &script)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Cannot make <{}> executable", path.display()))?;
        Ok(format!("Wrote <{}>", paths::display(&path)))
    }

    pub fn remove(self, host: Host) -> Result<Option<String>> {
        let path = self.path(host)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(Some(format!("Removed <{}>", paths::display(&path)))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("Cannot remove <{}>", path.display())),
        }
    }
}
