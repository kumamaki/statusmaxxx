#!/usr/bin/env bun
/** tuistory product QA for `statusmaxxx config`.

Runs the debug build against a sandbox HOME with four fake agents. The preview
draws a made-up repository and issue, so every screen is deterministic. Snapshot and screenshot after every action into qa-results/.
Asserts copy and chrome, not pixels.
*/

import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SESSION = "statusmaxxx-tui";
const COLS = 110;
// Tall enough for the whole Segments list below an action notice; 80x24 gets its own step.
const ROWS = 48;
const ROOT = join(import.meta.dir, "..");
const BINARY = join(ROOT, "target/debug/statusmaxxx");
const ARTIFACTS = join(ROOT, "qa-results");

const CODEX_CONFIG = ".codex/config.toml";
const CODEX_DEFAULT_ITEMS = ["current-dir", "git-branch", "model-with-reasoning", "context-used"];
const GEMINI_SETTINGS = ".gemini/settings.json";
const GEMINI_HAND_EDITED = `${JSON.stringify({ ui: { footer: { items: ["model-name"] } } })}\n`;
const CONFIG = ".config/statusmaxxx/config.toml";

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
  return [...screen.matchAll(/│ {3}(?:\S  |   )([A-Z][a-z]+(?: [a-z]+)?) +(?:◂ +)?(?:Shown|Hidden|Moving)/gu)].map((match) => match[1]);
}

/** The preview line: the header row that shows the sample worktree. */
function previewLine(screen: string): string {
  return screen.split("\n").find((row) => row.includes("shop:auth")) ?? "";
}

/** Whether the ↴ above the preview sits over the first cell of `start`. */
function points(screen: string, label: string, start: string): void {
  const pointer = screen.split("\n").find((row) => row.includes("↴"));
  const column = previewLine(screen).indexOf(start);
  if (pointer?.indexOf("↴") !== column) failed.push(`${label}: ↴ at ${pointer?.indexOf("↴")}, ${JSON.stringify(start)} at ${column}`);
}

function cardChrome(screen: string, label: string): void {
  must(screen, label, "╭─────────────────╮", "│   statusmaxxx   ╰", "╯  esc  │", "├", "╰");
}

/** HOME with six detected agents. Droid runs a status line script of its own, Codex
shows the items statusmaxxx gives the default segments, and Gemini a list edited by hand. */
async function fixture(): Promise<{ home: string; env: Record<string, string> }> {
  const home = await mkdtemp(join(tmpdir(), "statusmaxxx-qa-"));
  for (const directory of [".claude", ".factory", ".pi/agent", ".config/amp", ".codex", ".gemini"]) {
    await mkdir(join(home, directory), { recursive: true });
  }
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_CACHE_HOME: join(home, ".cache"),
  };
  const droidLine = { statusLine: { type: "command", command: join(home, ".factory/statusline.sh") } };
  await Bun.write(join(home, ".factory/settings.json"), JSON.stringify(droidLine));
  await Bun.write(join(home, CODEX_CONFIG), `[tui]\nstatus_line = ${JSON.stringify(CODEX_DEFAULT_ITEMS)}\n`);
  await Bun.write(join(home, GEMINI_SETTINGS), GEMINI_HAND_EDITED);
  return { home, env };
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
  const { home: sandbox, env } = await fixture();
  const sandboxFile = (path: string) => Bun.file(join(sandbox, path)).text();

  const envFlags = Object.entries(env).flatMap(([key, value]) => ["--env", `${key}=${value}`]);
  await sh(["tuistory", "launch", `${BINARY} config`, "-s", SESSION, "--cwd", sandbox, "--cols", String(COLS), "--rows", String(ROWS), "--background", "--timeout", "8000", ...envFlags]);

  try {
    await tui(["wait", "Segments", "--timeout", "8000"]);
    const home = await snapshot("home");
    must(home, "home", "Segments", "Agents", "Style", "Terminal theme · Dot separator", "Quit", "Choose what the line shows");
    mustNot(home, "home", "Look");
    must(home, "home", "shop/web", "shop:auth", "eng-42", "±3", "ENG-42 Fix login", "(In Progress)", "Opus", "42%");
    const homeStepper = stepper(home, "home");
    if (homeStepper.name !== "Claude Code") failed.push(`home: switcher shows ${JSON.stringify(homeStepper.name)}`);
    mustNot(home, "home", "Claude Code · Available", "other agents");
    must(home, "home", "1 installed · 5 available · 4 not found");
    cardChrome(home, "home");

    await press("right");
    const droid = await snapshot("home-droid");
    // Droid sends everything the default segments need; only cost, hidden, is missing.
    mustNot(droid, "home-droid", "doesn't report");
    must(previewLine(droid), "home-droid", "42%");
    const droidStepper = stepper(droid, "home-droid");
    if (droidStepper.name !== "Factory Droid") failed.push(`home-droid: switcher shows ${JSON.stringify(droidStepper.name)}`);
    if (droidStepper.left !== homeStepper.left || droidStepper.right !== homeStepper.right) {
      failed.push(`switcher arrows moved: Claude Code ${homeStepper.left}/${homeStepper.right}, Factory Droid ${droidStepper.left}/${droidStepper.right}`);
    }
    await press("left");

    await press("enter");
    const segments = await snapshot("segments");
    must(segments, "segments", "Every agent uses this list · Saved as you go", "Shown", "Hidden", "not in Amp", "◂ Shown", "[tab]", "All agents");
    if (rowOrder(segments).join(", ") !== "Directory, Worktree, Branch, Changes, Current issue, Model, Context, Session, Cost") {
      failed.push(`segments: rows are ${rowOrder(segments)}`);
    }
    cardChrome(segments, "segments");
    points(segments, "segments-pointer", "\uf07b");
    // The focused row's stepper paints its value in focus color; other rows stay unlit.
    const focusedControl = await tui(["snapshot", "--fg", "#ff453a", "--trim"], true);
    must(focusedControl, "segments-focused-control", "Directory", "Shown");
    mustNot(focusedControl, "segments-focused-control", "Worktree");

    await press("down", "down", "space");
    const hidden = await snapshot("segments-branch-hidden");
    mustNot(previewLine(hidden), "segments-branch-hidden", "eng-42");
    mustNot(hidden, "segments-branch-hidden", "↴");
    // Codex copies the list, so it follows; Gemini's hand-edited list stays.
    must(hidden, "segments-branch-hidden", "✓ Updated Codex CLI · new sessions show the change");
    must(await sandboxFile(CODEX_CONFIG), "segments-branch-hidden-codex", '"current-dir", "model-with-reasoning", "context-used"');
    mustNot(hidden, "segments-branch-hidden", "Updated Gemini");
    mustNot(await sandboxFile(CODEX_CONFIG), "segments-branch-hidden-codex", "git-branch");
    if ((await sandboxFile(GEMINI_SETTINGS)) !== GEMINI_HAND_EDITED) failed.push("segments-branch-hidden: Gemini settings changed");
    if (rowOrder(hidden).join() !== rowOrder(segments).join()) {
      failed.push(`segments-branch-hidden: rows moved from ${rowOrder(segments)} to ${rowOrder(hidden)}`);
    }
    await press("space");
    const restored = await snapshot("segments-branch-restored");
    must(previewLine(restored), "segments-branch-restored", "eng-42");
    must(await sandboxFile(CODEX_CONFIG), "segments-branch-restored-codex", '"git-branch"');
    points(restored, "segments-branch-pointer", "\ue725");
    if (rowOrder(restored).join() !== rowOrder(segments).join()) {
      failed.push(`segments-branch-restored: rows moved to ${rowOrder(restored)}`);
    }

    // Carry branch below changes: the list and the status line both follow.
    await press("m");
    must(await snapshot("segments-branch-picked-up"), "segments-branch-picked-up", "Moving", "enter puts it down");
    await press("down", "enter");
    const moved = await snapshot("segments-branch-moved");
    const order = rowOrder(moved);
    if (order.indexOf("Changes") > order.indexOf("Branch")) failed.push(`segments-branch-moved: order is ${order}`);
    const line = previewLine(moved);
    if (line.indexOf("±3") > line.indexOf("eng-42")) failed.push(`segments-branch-moved: preview is ${line.trim()}`);
    mustNot(moved, "segments-branch-moved", "Moving");
    await press("m", "up", "enter");

    // ←/→ flip shown and hidden on the focused row, like space.
    await press("right");
    mustNot(previewLine(await snapshot("segments-branch-arrow-hidden")), "segments-branch-arrow-hidden", "eng-42");
    await press("left");
    must(previewLine(await snapshot("segments-branch-arrow-shown")), "segments-branch-arrow-shown", "eng-42");

    // i turns only branch's icon off.
    const gitIcon = "\ue725";
    must(previewLine(restored), "segments-branch-icon-before", gitIcon);
    await press("i");
    const iconOff = await snapshot("segments-branch-icon-off");
    mustNot(previewLine(iconOff), "segments-branch-icon-off", gitIcon);
    must(previewLine(iconOff), "segments-branch-icon-off", "\uf1bb");
    await press("i");

    // tab scopes the screen to one agent; looking alone writes no override.
    await press("tab");
    const scoped = await snapshot("segments-scope-claude");
    must(scoped, "segments-scope-claude", "[tab]", "Claude Code", "uses the shared list · a change gives it its own", "tab switches agent", "i toggles its icon for every agent");
    mustNot(await sandboxFile(CONFIG), "segments-scope-claude-config", "[hosts.claude]");

    // The first edit gives the agent its own list; the shared one is untouched.
    await press("space");
    const own = await snapshot("segments-scope-claude-own");
    must(own, "segments-scope-claude-own", "Claude Code has its own list · r resets it to shared");
    mustNot(previewLine(own), "segments-scope-claude-own", "eng-42");
    const configWithOwn = await sandboxFile(CONFIG);
    must(configWithOwn, "segments-scope-claude-own-config", "[hosts.claude]");
    mustNot(configWithOwn.split("[hosts.claude]")[1] ?? "", "segments-scope-claude-own-config", '"branch"');

    // r drops it; the agent follows the shared list again.
    await press("r");
    const reset = await snapshot("segments-scope-claude-reset");
    must(reset, "segments-scope-claude-reset", "uses the shared list");
    must(previewLine(reset), "segments-scope-claude-reset", "eng-42");
    mustNot(await sandboxFile(CONFIG), "segments-scope-claude-reset-config", "[hosts.claude]");

    // tab keeps cycling through the detected agents.
    await press("tab");
    must(await snapshot("segments-scope-droid"), "segments-scope-droid", "Factory Droid uses the shared list");

    await press("esc", "down", "enter");
    const agents = await snapshot("agents");
    must(agents, "agents", "Claude Code", "Amp", "Codex CLI", "Available", "Not found", "Has its own status line (statusline.sh)");
    cardChrome(agents, "agents");

    await press("down", "down", "down", "down", "down");
    const amp = await snapshot("agents-amp");
    must(amp, "agents-amp", "Amp doesn't report its model and context");
    mustNot(amp, "agents-amp", "◂", "Amp · Available");
    mustNot(amp.split("\n").slice(0, 6).join("\n"), "agents-amp", "Opus");

    await press("enter");
    const agent = await snapshot("agent-amp");
    must(agent, "agent-amp", "experimental status item", "Install", "Uninstall");
    mustNot(agent, "agent-amp", "Shared", "Own list");
    await press("enter");
    await tui(["wait", "Reinstall", "--timeout", "5000"]);
    const installed = await snapshot("agent-amp-installed");
    must(installed, "agent-amp-installed", "Reinstall", "✓ Installed in Amp · new sessions show the line", "  Wrote <~/.config/amp/plugins/statusmaxxx.ts>");
    const rows = installed.split("\n");
    const headline = rows.findIndex((row) => row.includes("✓ Installed in Amp"));
    if (!rows[headline - 2]?.includes("├")) failed.push("agent-amp-installed: no rule above the notice");
    const green = await tui(["snapshot", "--fg", "#30d158", "--trim"], true);
    must(green, "agent-amp-installed-green", "✓ Installed in Amp");
    mustNot(green, "agent-amp-installed-green", "new sessions", "Wrote");
    cardChrome(installed, "agent-amp-installed");

    await press("esc");
    must(await snapshot("agents-installed"), "agents-installed", "Installed");
    const blue = await tui(["snapshot", "--fg", "#0a84ff", "--trim"], true);
    // Codex, kept in sync above, and Amp.
    const blueRows = blue.split("\n").map((row) => row.trim()).filter(Boolean);
    if (blueRows.join() !== "Installed,Installed") failed.push(`agents-after-install: blue text is ${JSON.stringify(blueRows)}`);

    await press("esc", "down", "enter", "right");
    const style = await snapshot("style");
    must(style, "style", "Saved as you go", "Theme", "Short Giraffe", "Separator", "Dot");
    mustNot(style, "style", "Icons");
    must(previewLine(style), "style", "eng-42 · ±3 · ");
    cardChrome(style, "style");

    // The divider shows in the preview, and both pickers keep their arrows in one column.
    await press("down", "right");
    const bar = await snapshot("style-separator-bar");
    must(bar, "style-separator-bar", "Bar");
    must(previewLine(bar), "style-separator-bar", "eng-42 │ ±3 │ ");
    const arrowColumns = bar.split("\n").filter((row) => /Theme|Separator/u.test(row)).map((row) => row.indexOf("◂"));
    if (new Set(arrowColumns).size !== 1) failed.push(`style-separator-bar: arrows at columns ${arrowColumns}`);

    await tui(["resize", "80", "24"]);
    await press("esc", "up", "up", "enter");
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
