mod config;
mod git;
mod host;
mod linear;
mod paths;
mod payload;
mod render;
mod segment;
mod theme;
mod tui;

use std::io::{self, IsTerminal, Read, Write};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::host::{Host, InstallState};
use crate::payload::Payload;

/// One status line for every coding agent.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Configure segments, theme, and agents interactively (the default).
    Config,
    /// Print the status line for an agent. Agents call this with session JSON on stdin.
    Render {
        #[arg(long)]
        host: Host,
    },
    /// Wire the status line into agents.
    Install {
        #[arg(required = true)]
        hosts: Vec<Host>,
    },
    /// Remove the status line from agents.
    Uninstall {
        #[arg(required = true)]
        hosts: Vec<Host>,
    },
    /// Show which agents are detected and wired up.
    Status,
    /// Linear issue cache.
    Linear {
        #[command(subcommand)]
        command: LinearCommand,
    },
}

#[derive(Subcommand)]
enum LinearCommand {
    /// Fetch issues into the cache. Reads the key from `LINEAR_API_KEY`.
    Refresh,
}

fn main() -> Result<()> {
    match Cli::parse().command.unwrap_or(Command::Config) {
        Command::Config => tui::run(),
        Command::Render { host } => render(host),
        Command::Install { hosts } => each_host(&hosts, |host, config| host.install(config)),
        Command::Uninstall { hosts } => each_host(&hosts, |host, _| host.uninstall()),
        Command::Status => status(),
        Command::Linear { command: LinearCommand::Refresh } => refresh_linear(),
    }
}

fn render(host: Host) -> Result<()> {
    let mut input = String::new();
    // Run by hand from a terminal there is no session JSON to wait for.
    if !io::stdin().is_terminal() {
        io::stdin().read_to_string(&mut input).context("Cannot read session JSON from stdin")?;
    }
    let session = Payload::parse(&input)?.into_session()?;
    if !input.trim().is_empty() {
        // The TUI previews each agent with the last session it really sent.
        paths::write_atomically(&paths::last_payload(host)?, &input)?;
    }
    let line = render::render(host, &Config::load()?, &session);
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{line}")?;
    stdout.flush().context("Cannot write the status line")
}

fn each_host(hosts: &[Host], action: impl Fn(Host, &Config) -> Result<Vec<String>>) -> Result<()> {
    let config = Config::load()?;
    for host in hosts {
        for line in action(*host, &config).with_context(|| format!("{} failed", host.label()))? {
            println!("{}: {line}", host.label());
        }
    }
    Ok(())
}

fn status() -> Result<()> {
    let config = Config::load()?;
    for host in Host::ALL {
        let state = if !host.detected()? {
            "not detected".to_string()
        } else {
            match host.state(&config)? {
                InstallState::Installed => "installed".to_string(),
                InstallState::NotInstalled => "not installed".to_string(),
                InstallState::Occupied(other) => format!("not installed; current: {other}"),
            }
        };
        println!("{:<14} {state}", host.label());
    }
    match linear::Cache::load()? {
        Some(cache) => println!(
            "\nLinear cache: {} started, {} branch issues, {} teams",
            cache.started.len(),
            cache.branch_issues.len(),
            cache.team_keys.len()
        ),
        None => println!("\nLinear cache: empty (run `statusmaxxx linear refresh`)"),
    }
    println!("Config: {}", paths::display(&paths::config_file()?));
    Ok(())
}

fn refresh_linear() -> Result<()> {
    let Some(api_key) = std::env::var("LINEAR_API_KEY").ok().filter(|key| !key.is_empty()) else {
        bail!(
            "LINEAR_API_KEY is not set; run it under your secret manager, e.g. `op run -- statusmaxxx linear refresh`"
        );
    };
    let cache = linear::refresh(&api_key)?;
    println!(
        "Cached <{}> started and <{}> branch issues across <{}> teams",
        cache.started.len(),
        cache.branch_issues.len(),
        cache.team_keys.len()
    );
    Ok(())
}
