mod config;
mod git;
mod host;
mod issue;
mod paths;
mod payload;
mod render;
mod segment;
mod separator;
mod theme;
mod tui;

use std::io::{self, IsTerminal, Read, Write};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use crate::config::Config;
use crate::git::Repo;
use crate::host::{Host, InstallState};
use crate::issue::{Issue, Issues};
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
    /// Agent hooks; `install` registers them.
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// Issues shown for the current worktree. Agents run these as they pick up and land work.
    Issue {
        #[command(subcommand)]
        command: IssueCommand,
    },
}

#[derive(Subcommand)]
enum HookEvent {
    /// Tell the agent which issue this worktree shows, or how to set one.
    SessionStart {
        #[arg(long)]
        host: Host,
    },
}

#[derive(Subcommand)]
enum IssueCommand {
    /// Show only this issue. Fields left out keep their stored value.
    Set(IssueArgs),
    /// Add an issue, or update it when its id is already set.
    Add(IssueArgs),
    /// Remove one issue, or all of them without an id.
    Clear { id: Option<String> },
    /// Print the issues set here.
    Show,
}

#[derive(Args)]
struct IssueArgs {
    /// Tracker id, e.g. ENG-42.
    id: String,
    title: Option<String>,
    #[arg(long)]
    state: Option<String>,
    #[arg(long)]
    url: Option<String>,
}

impl From<IssueArgs> for Issue {
    fn from(args: IssueArgs) -> Self {
        Issue { id: args.id, title: args.title, state: args.state, url: args.url }
    }
}

fn main() -> Result<()> {
    match Cli::parse().command.unwrap_or(Command::Config) {
        Command::Config => tui::run(),
        Command::Render { host } => render(host),
        Command::Install { hosts } => each_host(&hosts, |host, config| host.install(config)),
        Command::Uninstall { hosts } => each_host(&hosts, |host, _| host.uninstall()),
        Command::Status => status(),
        Command::Hook { event: HookEvent::SessionStart { host } } => session_start(host),
        Command::Issue { command } => issue(command),
    }
}

fn render(host: Host) -> Result<()> {
    let input = read_stdin()?;
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

/// Session JSON from the agent; run by hand from a terminal there is none to wait for.
fn read_stdin() -> Result<String> {
    let mut input = String::new();
    if !io::stdin().is_terminal() {
        io::stdin().read_to_string(&mut input).context("Cannot read session JSON from stdin")?;
    }
    Ok(input)
}

/// Outside a repository there is no worktree issue to talk about, so it prints nothing.
fn session_start(host: Host) -> Result<()> {
    let session = Payload::parse(&read_stdin()?)?.into_session()?;
    let Some(repo) = Repo::discover(&session.cwd)? else {
        return Ok(());
    };
    let briefing = Issues::of(&repo)?.briefing();
    println!("{}", host::hook::output(host, &briefing));
    Ok(())
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
    println!("\nConfig: {}", paths::display(&paths::config_file()?));
    Ok(())
}

fn issue(command: IssueCommand) -> Result<()> {
    let cwd = std::env::current_dir().context("Cannot resolve the working directory")?;
    let Some(repo) = Repo::discover(&cwd)? else {
        bail!("Issues are kept per worktree, and <{}> is not in a git repository", cwd.display());
    };
    let mut issues = Issues::of(&repo)?;
    match command {
        IssueCommand::Set(args) => issues.set(args.into()),
        IssueCommand::Add(args) => issues.add(args.into()),
        IssueCommand::Clear { id: Some(id) } => issues.remove(&id)?,
        IssueCommand::Clear { id: None } => issues.clear(),
        IssueCommand::Show => {
            for issue in &issues.list {
                let details: Vec<&str> =
                    [&issue.title, &issue.state, &issue.url].into_iter().flatten().map(String::as_str).collect();
                println!("{}  {}", issue.id, details.join(" · "));
            }
            return Ok(());
        }
    }
    issues.save()
}
