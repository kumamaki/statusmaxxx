# statusmaxxx

One status line for every coding agent. You configure it once, and each agent shows the same segments: worktree, git, the issue the agent is working on, model, context, and cost.

```
 app:app-auth   mehdi/eng-42-fix-auth ±1   ENG-42 Fix the auth flow (In Review) +2  󰚩 Opus 5.5   42%
```

## Install

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/kumamaki/statusmaxxx/releases/latest/download/statusmaxxx-installer.sh | sh
# or from source
cargo install --git https://github.com/kumamaki/statusmaxxx
```

Icons need a [Nerd Font](https://www.nerdfonts.com). Turn them off with `n` in the TUI or `icons = false` in the config.

## Use

```sh
statusmaxxx                   # TUI: segments, theme, per-agent overrides, install
statusmaxxx install claude amp
statusmaxxx status
statusmaxxx uninstall amp
```

`install` saves whatever status line it replaces, and `uninstall` puts it back. Every settings file it edits also gets a `<file>.statusmaxxx.bak` copy. `install` also adds a short marked instruction to the agent's global instructions file, telling it to set its issue (see [Issues](#issues)); `uninstall` removes it.

## Agents

| Agent | How it hooks in | What it edits |
|---|---|---|
| Claude Code | `statusLine` command | `~/.claude/settings.json` |
| Cursor CLI | `statusLine` command (replaces Cursor's footer) | `~/.cursor/cli-config.json` |
| Qwen Code | `ui.statusLine` command | `~/.qwen/settings.json` |
| Factory Droid | `statusLine` command | `~/.factory/settings.json` |
| Copilot CLI | `statusLine` command + `footer.showCustom` | `~/.copilot/settings.json` |
| Amp | plugin on the experimental status item API | `~/.config/amp/plugins/statusmaxxx.ts` |
| pi | extension status in the footer | `~/.pi/agent/extensions/statusmaxxx.ts` |
| OpenCode | TUI plugin in the `app_bottom` slot | `~/.config/opencode/tui.json`, `tui-plugins/` |
| Codex CLI | its own `tui.status_line` items | `~/.codex/config.toml` |
| Gemini CLI | its own `ui.footer.items` | `~/.gemini/settings.json` |

Codex and Gemini cannot show custom text, so they only get the segments they have an item for. For them, worktree and issue are not available.

Command agents run a small wrapper script in `~/.config/statusmaxxx/hosts/`. It calls `statusmaxxx render --host <agent>` with the session JSON on stdin. Plugin agents run a generated shim that calls the same command.

## Config

`~/.config/statusmaxxx/config.toml`, written by the TUI:

```toml
segments = ["worktree", "git", "issue", "model", "context"]
theme = "terminal"          # terminal, catppuccin, dracula, nord, gruvbox, light
icons = true
separator = "  "

[hosts.amp]                 # Amp shows its own model, so it gets its own list
segments = ["worktree", "git", "issue"]
```

Segments: `directory`, `worktree`, `git`, `issue`, `model`, `context`, `cost`. When an agent does not send something (Droid has no cost, for example), that segment stays empty.

## Issues

The agent knows which issue it is working on, because it read it from the tracker, so it sets the issue itself:

```sh
statusmaxxx issue set ENG-42 "Fix the auth flow" --state "In Progress" --url https://linear.app/…
statusmaxxx issue set ENG-42 --state "In Review"     # fields left out keep their value
statusmaxxx issue add ENG-43                          # more than one issue
statusmaxxx issue clear                               # or: clear ENG-43
statusmaxxx issue show
```

Issues are kept per worktree, in that worktree's own git dir. They survive restarts, every linked worktree has its own list, and nothing shows up in `git status`. Any tracker works: the id is just text.

`install` teaches the agent to do this by adding a marked block to its global instructions:

| Agent | File |
|---|---|
| Claude Code | `~/.claude/CLAUDE.md` |
| Qwen Code | `~/.qwen/QWEN.md` |
| Droid, Amp, pi, OpenCode | their global `AGENTS.md` |
| Copilot CLI | `~/.copilot/instructions/statusmaxxx.instructions.md` |

Cursor has no global instructions file, so add the same line to its user rules yourself.

## Debugging

A segment that fails shows `✗ <segment>` and writes the reason to stderr. To see it, replay the last session an agent sent:

```sh
statusmaxxx render --host claude < ~/.cache/statusmaxxx/payloads/claude.json
```
