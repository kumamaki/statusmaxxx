#!/usr/bin/env bun
/** tuistory product QA for `statusmaxxx config`.

Runs the debug build against a sandbox HOME (four fake agents) and a fixture
repository with a linked worktree and an issue, so every screen is
deterministic. Snapshot and screenshot after every action into qa-results/.
Asserts copy and chrome, not pixels.
*/

import { mkdir, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SESSION = "statusmaxxx-tui";
const COLS = 110;
const ROWS = 40;
const ROOT = join(import.meta.dir, "..");
const BINARY = join(ROOT, "target/debug/statusmaxxx");
const ARTIFACTS = join(ROOT, "qa-results");

const failed: string[] = [];
let step = 0;

async function sh(args: string[], options: { cwd?: string; env?: Record<string, string>; allowFail?: boolean } = {}) {
  const proc = Bun.spawn(args, {
    cwd: options.cwd ?? ROOT,
    env: { ...process.env, ...options.env },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr] = await Promise.all([new Response(proc.stdout).text(), new Response(proc.stderr).text()]);
  const code = await proc.exited;
  const output = (stdout + stderr).trim();
  if (code !== 0 && !options.allowFail) throw new Error(`${args.join(" ")}\n${output}`);
  return output;
}

const tui = (args: string[], allowFail = false) => sh(["tuistory", "-s", SESSION, ...args], { allowFail });

async function press(...keys: string[]): Promise<void> {
  for (const key of keys) await tui(["press", key]);
}

async function snapshot(label: string): Promise<string> {
  await tui(["wait-idle", "--timeout", "3000"], true);
  step += 1;
  const name = `${String(step).padStart(2, "0")}-${label}`;
  const screen = await tui(["snapshot", "--trim"]);
  await Bun.write(join(ARTIFACTS, `${name}.txt`), `${screen}\n`);
  await tui(["screenshot", "-o", join(ARTIFACTS, `${name}.png`), "--pixel-ratio", "2"], true);
  return screen;
}

function must(screen: string, label: string, ...needles: string[]): void {
  for (const needle of needles) if (!screen.includes(needle)) failed.push(`${label}: missing ${JSON.stringify(needle)}`);
}

function mustNot(screen: string, label: string, ...needles: string[]): void {
  for (const needle of needles) if (screen.includes(needle)) failed.push(`${label}: still shows ${JSON.stringify(needle)}`);
}

function cardChrome(screen: string, label: string): void {
  must(screen, label, "╭─────────────────╮", "│   statusmaxxx   ╰", "╯  esc  │", "├", "╰");
}

/** HOME with four detected agents, and a repo whose linked worktree has an issue set. */
async function fixture(): Promise<{ home: string; worktree: string; env: Record<string, string> }> {
  const home = await mkdtemp(join(tmpdir(), "statusmaxxx-qa-"));
  for (const directory of [".claude", ".factory", ".pi/agent", ".config/amp"]) {
    await mkdir(join(home, directory), { recursive: true });
  }
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_CACHE_HOME: join(home, ".cache"),
  };
  const repo = join(home, "src/app");
  const worktree = join(home, "src/app-auth");
  await mkdir(repo, { recursive: true });
  await sh(["git", "init", "-q", "-b", "main"], { cwd: repo });
  await sh(["git", "-c", "user.name=qa", "-c", "user.email=qa@example.com", "commit", "-q", "--allow-empty", "-m", "init"], { cwd: repo });
  await sh(["git", "worktree", "add", "-q", "-b", "smx-1-auth", worktree], { cwd: repo });
  await sh([BINARY, "issue", "set", "SMX-1", "Fix the auth flow", "--state", "In Progress"], { cwd: worktree, env });
  return { home, worktree, env };
}

async function main(): Promise<number> {
  if ((await sh(["which", "tuistory"], { allowFail: true })) === "") {
    console.error("BLOCKED: tuistory not on PATH");
    return 2;
  }
  await mkdir(ARTIFACTS, { recursive: true });
  await tui(["close"], true);
  const { worktree, env } = await fixture();

  const envFlags = Object.entries(env).flatMap(([key, value]) => ["--env", `${key}=${value}`]);
  await sh(["tuistory", "launch", `${BINARY} config`, "-s", SESSION, "--cwd", worktree, "--cols", String(COLS), "--rows", String(ROWS), "--background", "--timeout", "8000", ...envFlags]);

  try {
    await tui(["wait", "Segments", "--timeout", "8000"]);
    const home = await snapshot("home");
    must(home, "home", "Segments", "Agents", "Look", "Quit", "Choose what the line shows");
    must(home, "home", "app:app-auth", "smx-1-auth", "SMX-1 Fix the auth flow", "(In Progress)", "Opus", "42%");
    must(home, "home", "0 installed · 4 available · 6 not found");
    cardChrome(home, "home");

    await press("enter");
    const segments = await snapshot("segments");
    must(segments, "segments", "shared by all agents", "shown", "hidden", "not in Amp");
    cardChrome(segments, "segments");

    await press("down", "space");
    const hidden = await snapshot("segments-git-hidden");
    mustNot(hidden.split("\n").slice(0, 6).join("\n"), "segments-git-hidden", "smx-1-auth");
    await press("space");
    const restored = await snapshot("segments-git-restored");
    must(restored.split("\n").slice(0, 6).join("\n"), "segments-git-restored", "smx-1-auth");

    await press("esc", "down", "enter");
    const agents = await snapshot("agents");
    must(agents, "agents", "Claude Code", "Amp", "Codex CLI", "available", "not found");
    cardChrome(agents, "agents");

    await press("down", "down", "down", "down", "down");
    const amp = await snapshot("agents-amp");
    must(amp, "agents-amp", "Amp · available", "Can't show here: model · context");
    mustNot(amp.split("\n").slice(0, 6).join("\n"), "agents-amp", "Opus");

    await press("enter");
    const agent = await snapshot("agent-amp");
    must(agent, "agent-amp", "experimental status item", "Install", "Uninstall", "Segments", "Shared");
    await press("enter");
    await tui(["wait", "Reinstall", "--timeout", "5000"]);
    const installed = await snapshot("agent-amp-installed");
    must(installed, "agent-amp-installed", "Amp · installed", "Wrote <~/.config/amp/plugins/statusmaxxx.ts>");
    cardChrome(installed, "agent-amp-installed");

    await press("esc", "esc", "down", "enter", "right");
    const look = await snapshot("look");
    must(look, "look", "Theme", "short-giraffe", "Icons", "On", "Off");
    cardChrome(look, "look");

    await tui(["resize", "80", "24"]);
    await press("esc", "enter");
    const narrow = await snapshot("segments-80x24");
    cardChrome(narrow, "segments-80x24");
  } finally {
    await tui(["close"], true);
  }

  if (failed.length > 0) {
    console.error(`FAIL  ${failed.length} assertion(s)`);
    for (const line of failed) console.error(`  - ${line}`);
    console.error(`snapshots in ${ARTIFACTS}`);
    return 1;
  }
  console.log(`PASS  ${step} snapshots → ${ARTIFACTS}`);
  return 0;
}

process.exit(await main());
