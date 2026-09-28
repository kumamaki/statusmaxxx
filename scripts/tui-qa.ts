#!/usr/bin/env bun
/** tuistory product QA for `statusmaxxx config`.

Runs the debug build against a sandbox HOME (four fake agents) and a fixture
repository with a linked worktree and an issue, so every screen is
deterministic. Snapshot and screenshot after every action into qa-results/.
Asserts copy and chrome, not pixels.
*/

import { mkdir, mkdtemp, rm } from "node:fs/promises";
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

/** The agent switcher: its label and the columns of both arrows. */
function stepper(screen: string, label: string): { name: string; left: number; right: number } {
  const line = screen.split("\n").find((row) => row.includes("◂") && row.includes("▸"));
  if (!line) {
    failed.push(`${label}: no agent switcher`);
    return { name: "", left: -1, right: -1 };
  }
  const left = line.indexOf("◂");
  const right = line.indexOf("▸");
  return { name: line.slice(left + 1, right).trim(), left, right };
}

/** Segment names in the order the Segments screen lists them. */
function rowOrder(screen: string): string[] {
  return [...screen.matchAll(/│ {3}(?:\S  |   )([a-z]+) +(?:◂ +)?(?:shown|hidden|moving)/gu)].map((match) => match[1]);
}

/** The preview line: the first header row. */
function previewLine(screen: string): string {
  return screen.split("\n")[3] ?? "";
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
  // Captures from an earlier run with more steps would pass for this run's.
  await rm(ARTIFACTS, { recursive: true, force: true });
  await mkdir(ARTIFACTS, { recursive: true });
  await tui(["close"], true);
  const { worktree, env } = await fixture();

  const envFlags = Object.entries(env).flatMap(([key, value]) => ["--env", `${key}=${value}`]);
  await sh(["tuistory", "launch", `${BINARY} config`, "-s", SESSION, "--cwd", worktree, "--cols", String(COLS), "--rows", String(ROWS), "--background", "--timeout", "8000", ...envFlags]);

  try {
    await tui(["wait", "Segments", "--timeout", "8000"]);
    const home = await snapshot("home");
    must(home, "home", "Segments", "Agents", "Style", "terminal theme · space separator", "Quit", "Choose what the line shows");
    mustNot(home, "home", "Look");
    must(home, "home", "app:app-auth", "smx-1-auth", "SMX-1 Fix the auth flow", "(In Progress)", "Opus", "42%");
    const homeStepper = stepper(home, "home");
    if (homeStepper.name !== "Claude Code") failed.push(`home: switcher shows ${JSON.stringify(homeStepper.name)}`);
    mustNot(home, "home", "Claude Code · available", "other agents");
    must(home, "home", "0 installed · 4 available · 6 not found");
    cardChrome(home, "home");

    await press("right");
    const droid = await snapshot("home-droid");
    must(droid, "home-droid", "Factory Droid doesn't report its context");
    const droidStepper = stepper(droid, "home-droid");
    if (droidStepper.name !== "Factory Droid") failed.push(`home-droid: switcher shows ${JSON.stringify(droidStepper.name)}`);
    if (droidStepper.left !== homeStepper.left || droidStepper.right !== homeStepper.right) {
      failed.push(`switcher arrows moved: Claude Code ${homeStepper.left}/${homeStepper.right}, Factory Droid ${droidStepper.left}/${droidStepper.right}`);
    }
    await press("left");

    await press("enter");
    const segments = await snapshot("segments");
    must(segments, "segments", "Every agent uses this list", "shown", "hidden", "not in Amp", "◂ shown");
    if (rowOrder(segments).join(" ") !== "worktree branch changes issue model context directory cost") {
      failed.push(`segments: rows are ${rowOrder(segments)}`);
    }
    cardChrome(segments, "segments");

    await press("down", "space");
    const hidden = await snapshot("segments-branch-hidden");
    mustNot(previewLine(hidden), "segments-branch-hidden", "smx-1-auth");
    if (rowOrder(hidden).join() !== rowOrder(segments).join()) {
      failed.push(`segments-branch-hidden: rows moved from ${rowOrder(segments)} to ${rowOrder(hidden)}`);
    }
    await press("space");
    const restored = await snapshot("segments-branch-restored");
    must(previewLine(restored), "segments-branch-restored", "smx-1-auth");
    if (rowOrder(restored).join() !== rowOrder(segments).join()) {
      failed.push(`segments-branch-restored: rows moved to ${rowOrder(restored)}`);
    }

    // Carry branch below changes: the list and the status line both follow.
    await press("m");
    must(await snapshot("segments-branch-picked-up"), "segments-branch-picked-up", "moving", "enter puts it down");
    await press("down", "enter");
    const moved = await snapshot("segments-branch-moved");
    const order = rowOrder(moved);
    if (order.indexOf("changes") > order.indexOf("branch")) failed.push(`segments-branch-moved: order is ${order}`);
    const line = previewLine(moved);
    if (line.indexOf("✓") > line.indexOf("smx-1-auth")) failed.push(`segments-branch-moved: preview is ${line.trim()}`);
    mustNot(moved, "segments-branch-moved", "moving");
    await press("m", "up", "enter");

    // ←/→ flip shown and hidden on the focused row, like space.
    await press("right");
    mustNot(previewLine(await snapshot("segments-branch-arrow-hidden")), "segments-branch-arrow-hidden", "smx-1-auth");
    await press("left");
    must(previewLine(await snapshot("segments-branch-arrow-shown")), "segments-branch-arrow-shown", "smx-1-auth");

    // i turns only branch's icon off.
    const gitIcon = "\ue725";
    must(previewLine(restored), "segments-branch-icon-before", gitIcon);
    await press("i");
    const iconOff = await snapshot("segments-branch-icon-off");
    mustNot(previewLine(iconOff), "segments-branch-icon-off", gitIcon);
    must(previewLine(iconOff), "segments-branch-icon-off", "\uf1bb");
    await press("i");

    await press("esc", "down", "enter");
    const agents = await snapshot("agents");
    must(agents, "agents", "Claude Code", "Amp", "Codex CLI", "available", "not found");
    cardChrome(agents, "agents");

    await press("down", "down", "down", "down", "down");
    const amp = await snapshot("agents-amp");
    must(amp, "agents-amp", "Amp doesn't report its model and context");
    mustNot(amp, "agents-amp", "◂", "Amp · available");
    mustNot(amp.split("\n").slice(0, 6).join("\n"), "agents-amp", "Opus");

    await press("enter");
    const agent = await snapshot("agent-amp");
    must(agent, "agent-amp", "experimental status item", "Install", "Uninstall", "Segments", "Shared");
    await press("enter");
    await tui(["wait", "Reinstall", "--timeout", "5000"]);
    const installed = await snapshot("agent-amp-installed");
    must(installed, "agent-amp-installed", "Reinstall", "Wrote <~/.config/amp/plugins/statusmaxxx.ts>");
    cardChrome(installed, "agent-amp-installed");

    await press("esc", "esc", "down", "enter", "right");
    const style = await snapshot("style");
    must(style, "style", "Theme", "short-giraffe", "Separator", "space");
    mustNot(style, "style", "Icons");
    mustNot(previewLine(style), "style", "·");
    cardChrome(style, "style");

    // The divider shows in the preview, and both pickers keep their arrows in one column.
    await press("down", "right");
    const dot = await snapshot("style-separator-dot");
    must(dot, "style-separator-dot", "dot");
    must(previewLine(dot), "style-separator-dot", "smx-1-auth · ✓ · ");
    const arrowColumns = dot.split("\n").filter((row) => /Theme|Separator/u.test(row)).map((row) => row.indexOf("◂"));
    if (new Set(arrowColumns).size !== 1) failed.push(`style-separator-dot: arrows at columns ${arrowColumns}`);

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
