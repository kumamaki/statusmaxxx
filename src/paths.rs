use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::host::Host;

const APP: &str = "statusmaxxx";

pub fn home() -> Result<PathBuf> {
    dirs::home_dir().context("Cannot resolve the home directory")
}

// XDG-style locations on every platform: agents keep their own configs under
// `~/.config` too, and `~/Library/Application Support` would hide ours.
pub fn config_dir() -> Result<PathBuf> {
    Ok(env_or_home("XDG_CONFIG_HOME", ".config")?.join(APP))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(env_or_home("XDG_CACHE_HOME", ".cache")?.join(APP))
}

pub fn xdg_config_home() -> Result<PathBuf> {
    env_or_home("XDG_CONFIG_HOME", ".config")
}

/// `variable` set to a non-empty value wins; otherwise `~/<fallback>`.
pub fn env_or_home(variable: &str, fallback: &str) -> Result<PathBuf> {
    match env::var_os(variable) {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Ok(home()?.join(fallback)),
    }
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn replaced_record(host: Host) -> Result<PathBuf> {
    Ok(config_dir()?.join("replaced").join(format!("{}.json", host.id())))
}

pub fn last_payload(host: Host) -> Result<PathBuf> {
    Ok(cache_dir()?.join("payloads").join(format!("{}.json", host.id())))
}

/// The last session-start payload, beside the status line's.
pub fn last_hook_payload(host: Host) -> Result<PathBuf> {
    Ok(cache_dir()?.join("payloads").join(format!("{}-hook.json", host.id())))
}

/// The path agents should call. Prefers the `PATH` entry over the resolved
/// executable so package-manager upgrades (versioned cellar paths) keep working.
pub fn binary() -> Result<PathBuf> {
    let executable = env::current_exe().context("Cannot resolve the running executable")?;
    let resolved = fs::canonicalize(&executable)?;
    let on_path = env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(APP))
        .find(|candidate| fs::canonicalize(candidate).is_ok_and(|target| target == resolved));
    Ok(on_path.unwrap_or(resolved))
}

pub fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Cannot create <{}>", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("Cannot write <{}>", path.display()))
}

/// For files renders read while another process rewrites them.
pub fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    write_file(&temporary, contents)?;
    fs::rename(&temporary, path).with_context(|| format!("Cannot replace <{}>", path.display()))
}

pub fn display(path: &Path) -> String {
    match home() {
        Ok(home) => match path.strip_prefix(&home) {
            Ok(relative) if relative.as_os_str().is_empty() => "~".to_string(),
            Ok(relative) => format!("~/{}", relative.display()),
            Err(_) => path.display().to_string(),
        },
        Err(_) => path.display().to_string(),
    }
}
