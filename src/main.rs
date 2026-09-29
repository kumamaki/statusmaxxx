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
mod worktree;

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

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
    /// The worktree repository segments follow. Agents set it when their work
    /// happens in another checkout than the one they run in.
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
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
enum WorktreeCommand {
    /// Follow the repository at <path>; every repository segment shows it.
    Set { path: PathBuf },
    /// Follow this checkout's own repository again.
    Clear,
    /// Print the worktree the line follows.
    Show,
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
        Command::Uninstall { hosts } => each_host(&hosts, |host, config| host.uninstall(config)),
        Command::Status => status(),
        Command::Hook { event: HookEvent::SessionStart { host } } => session_start(host),
        Command::Issue { command } => issue(command),
        Command::Worktree { command } => worktree(command),
    }
}

fn render(host: Host) -> Result<()> {
    let input = read_stdin()?;
    // A bad payload or config degrades to defaults and reports itself in the
    // line — failures show as `✗`, never as a blank status line.
    let mut problems = Vec::new();
    let mut session = match Payload::parse(&input).and_then(Payload::into_session) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("[statusmaxxx] {error:#}");
            problems.push("payload".to_string());
            Payload::default().into_session()?
        }
    };
    // The agent's own registry names the session as peers see it; that name
    // beats the title the payload carries.
    session.session_name = session.session_id.as_deref().and_then(|id| host.session_name(id)).or(session.session_name);
    // Kept for debugging and `just render` replay; a session that carries an id
    // also records its own file, so concurrent sessions cannot hide each other.
    record_payload(paths::last_payload(host)?, &input);
    if let Some(id) = session.session_id.as_deref().filter(|id| !id.is_empty()) {
        record_payload(paths::session_payload(host, id)?, &input);
    }
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[statusmaxxx] {error:#}");
            problems.push("config".to_string());
            Config::default()
        }
    };
    let line = render::render(host, &config, &session, &problems);
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{line}")?;
    stdout.flush().context("Cannot write the status line")
}

/// The last thing the agent sent on a path, kept for debugging and replay.
/// Identical refreshes are not rewritten, and a failed write only reports
/// itself — a debug record must never take the line down with it.
fn record_payload(path: PathBuf, input: &str) {
    if input.trim().is_empty() || std::fs::read_to_string(&path).ok().as_deref() == Some(input) {
        return;
    }
    if let Err(error) = paths::write_atomically(&path, input) {
        eprintln!("[statusmaxxx] {error:#}");
    }
}

/// Session JSON from the agent; run by hand from a terminal there is none to wait for.
fn read_stdin() -> Result<String> {
    let mut input = String::new();
    if !io::stdin().is_terminal() {
        io::stdin().read_to_string(&mut input).context("Cannot read session JSON from stdin")?;
    }
    Ok(input)
}

/// Outside a repository there is no worktree context to talk about, so it prints nothing.
fn session_start(host: Host) -> Result<()> {
    let input = read_stdin()?;
    // Recorded before parsing so even a malformed payload can be inspected.
    record_payload(paths::last_hook_payload(host)?, &input);
    let session = Payload::parse(&input)?.into_session()?;
    if let Some(id) = session.session_id.as_deref().filter(|id| !id.is_empty()) {
        record_payload(paths::session_hook_payload(host, id)?, &input);
    }
    let Some(repo) = Repo::discover(&session.cwd)? else {
        return Ok(());
    };
    let briefing = format!("{} {}", worktree::briefing(repo.declared.as_deref()), Issues::of(&repo)?.briefing());
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
        println!("{:<14} {}", host.label(), state_of(host, &config));
    }
    println!("\nConfig: {}", paths::display(&paths::config_file()?));
    Ok(())
}

/// One host's install state. A failure reads as `error: …` in place instead of
/// hiding every other host's state behind the first problem.
fn state_of(host: Host, config: &Config) -> String {
    match host.detected() {
        Ok(false) => return "not detected".to_string(),
        Err(error) => return format!("error: {error:#}"),
        Ok(true) => {}
    }
    match host.state(config) {
        Ok(InstallState::Installed) => "installed".to_string(),
        Ok(InstallState::NotInstalled) => "not installed".to_string(),
        Ok(InstallState::Occupied(other)) => format!("not installed; current: {other}"),
        Err(error) => format!("error: {error:#}"),
    }
}

/// The marker lives in this checkout's own git dir, so `at` — not `discover` —
/// finds it even when a declared worktree already redirects the context.
fn worktree(command: WorktreeCommand) -> Result<()> {
    let cwd = std::env::current_dir().context("Cannot resolve the working directory")?;
    let Some(repo) = Repo::at(&cwd)? else {
        bail!("The status line follows the checkout the agent runs in, and <{}> is not in a repository", cwd.display());
    };
    match command {
        WorktreeCommand::Set { path } => {
            // Canonicalized so the marker holds the real path, not the spelling typed.
            let path = cwd.join(&path);
            let path = std::fs::canonicalize(&path).with_context(|| format!("Cannot resolve <{}>", path.display()))?;
            if path == repo.root {
                bail!("<{}> is this checkout — nothing to follow", paths::display(&path));
            }
            if Repo::at(&path)?.is_none() {
                bail!("<{}> is not in a git repository", paths::display(&path));
            }
            worktree::set(&repo.git_dir, &path)
        }
        WorktreeCommand::Clear => worktree::clear(&repo.git_dir),
        WorktreeCommand::Show => {
            if let Some(path) = worktree::declared(&repo.git_dir)? {
                println!("{}", paths::display(&path));
            }
            Ok(())
        }
    }
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
                let details = [&issue.title, &issue.state, &issue.url]
                    .into_iter()
                    .flatten()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(" · ");
                if details.is_empty() {
                    println!("{}", issue.id);
                } else {
                    println!("{}  {details}", issue.id);
                }
            }
            return Ok(());
        }
    }
    issues.save()
}
