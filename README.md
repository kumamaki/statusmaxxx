# statusmaxxx

One status line for every coding agent. You configure it once, and each agent shows the same segments: worktree, git, the Linear issue you are on, model, context, and cost.

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

`install` saves whatever status line it replaces, and `uninstall` puts it back. Every settings file it edits also gets a `<file>.statusmaxxx.bak` copy.

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

Codex and Gemini cannot show custom text, so they only get the segments they have an item for. For them, worktree and Linear are not available.

Command agents run a small wrapper script in `~/.config/statusmaxxx/hosts/`. It calls `statusmaxxx render --host <agent>` with the session JSON on stdin. Plugin agents run a generated shim that calls the same command.

## Config

`~/.config/statusmaxxx/config.toml`, written by the TUI:

```toml
segments = ["worktree", "git", "linear", "model", "context"]
theme = "terminal"          # terminal, catppuccin, dracula, nord, gruvbox, light
icons = true
separator = "  "

[hosts.amp]                 # Amp shows its own model, so it gets its own list
segments = ["worktree", "git", "linear"]
```

Segments: `directory`, `worktree`, `git`, `linear`, `model`, `context`, `cost`. When an agent does not send something (Droid has no cost, for example), that segment stays empty.

## Linear

Status lines re-run every few hundred milliseconds, so `render` only reads a local cache. `statusmaxxx linear refresh` fills it with these:

- Your issues in a started state.
- Every issue key a branch showed in the last week. `mehdi/eng-42-fix-auth` becomes `ENG-42`, and only for teams you belong to.

The refresh reads a [personal API key](https://linear.app/settings/account/security) from `LINEAR_API_KEY`:

```sh
op run --env-file=~/.env.secrets -- statusmaxxx linear refresh
```

To run it every five minutes, use `~/Library/LaunchAgents/dev.statusmaxxx.linear.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.statusmaxxx.linear</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/zsh</string><string>-lc</string>
    <string>op run --env-file="$HOME/.env.secrets" -- statusmaxxx linear refresh</string>
  </array>
  <key>StartInterval</key><integer>300</integer>
</dict>
</plist>
```

Load it with `launchctl load ~/Library/LaunchAgents/dev.statusmaxxx.linear.plist`. For `op` to work unattended, the 1Password app must be unlocked.

## Debugging

A segment that fails shows `✗ <segment>` and writes the reason to stderr. To see it, replay the last session an agent sent:

```sh
statusmaxxx render --host claude < ~/.cache/statusmaxxx/payloads/claude.json
```
