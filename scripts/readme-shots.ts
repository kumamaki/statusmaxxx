#!/usr/bin/env bun
/** README screenshots into docs/: a real rendered status line, then the TUI's screens.

Runs the debug build against a sandbox HOME and a scratch repository with a
linked worktree, so the captures show the same session the TUI previews.
*/

import { mkdir, mkdtemp, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SESSION = "statusmaxxx-readme";
const ROOT = join(import.meta.dir, "..");
const BINARY = join(ROOT, "target/debug/statusmaxxx");
const DOCS = join(ROOT, "docs");

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

async function shot(name: string): Promise<void> {
  await tui(["wait-idle", "--timeout", "3000"]);
  await tui(["screenshot", "-o", join(DOCS, name), "--pixel-ratio", "2"]);
  console.log(`docs/${name}`);
}

/** Sandbox HOME, a config with every segment, and the preview's repository:
`shop` with a linked worktree `auth` on branch eng-42, three changed files,
and an issue set, all under a `web` folder the agent would run in. */
async function fixture(): Promise<{ home: string; cwd: string; env: Record<string, string> }> {
  const home = await mkdtemp(join(tmpdir(), "statusmaxxx-shots-"));
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_CACHE_HOME: join(home, ".cache"),
  };
  for (const directory of [".claude", ".factory", ".pi/agent", ".config/amp", ".config/statusmaxxx", ".codex", ".gemini"]) {
    await mkdir(join(home, directory), { recursive: true });
  }
  await Bun.write(join(home, ".config/statusmaxxx/config.toml"), "icons = true\n");
  // Detected in the Agents screen: Droid runs a foreign status line, Codex has
  // a statusmaxxx item list, Gemini a hand-edited one.
  await Bun.write(
    join(home, ".factory/settings.json"),
    JSON.stringify({ statusLine: { type: "command", command: join(home, ".factory/statusline.sh") } }),
  );
  await Bun.write(
    join(home, ".codex/config.toml"),
    '[tui]\nstatus_line = ["current-dir", "git-branch", "model-with-reasoning", "context-used"]\n',
  );
  await Bun.write(join(home, ".gemini/settings.json"), `${JSON.stringify({ ui: { footer: { items: ["model-name"] } } })}\n`);

  const shop = join(home, "src/shop");
  await mkdir(join(shop, "web"), { recursive: true });
  const git = (args: string[], cwd = shop) => sh(["git", "-C", cwd, ...args]);
  await sh(["git", "init", "-q", "-b", "main", shop]);
  await git(["config", "user.email", "shots@example.com"]);
  await git(["config", "user.name", "Shots"]);
  await Bun.write(join(shop, "web/app.ts"), "export {};\n");
  await git(["add", "."]);
  await git(["commit", "-qm", "init"]);
  await git(["worktree", "add", "-q", join(home, "src/auth"), "-b", "eng-42"]);
  const auth = join(home, "src/auth");
  for (const file of ["web/app.ts", "web/b.ts", "web/c.ts"]) await Bun.write(join(auth, file), "export {};\n// change\n");
  await sh([BINARY, "issue", "set", "ENG-42", "Fix login", "--state", "In Progress"], { cwd: auth, env });
  // git rev-parse resolves /var to /private/var; the payload cwd must match.
  return { home, cwd: await realpath(join(auth, "web")), env };
}

async function main(): Promise<number> {
  if ((await sh(["which", "tuistory"], { allowFail: true })) === "") {
    console.error("BLOCKED: tuistory not on PATH");
    return 2;
  }
  await mkdir(DOCS, { recursive: true });
  const { home, cwd, env } = await fixture();
  const envFlags = Object.entries(env).flatMap(([key, value]) => ["--env", `${key}=${value}`]);

  // The status line an agent's status area would show, rendered for real.
  const payload = {
    cwd,
    model: { display_name: "Opus" },
    context_window: { used_percentage: 42 },
  };
  const runner = join(home, "render.sh");
  await Bun.write(
    runner,
    `#!/bin/sh\n${BINARY} render --host claude <<'EOF'\n${JSON.stringify(payload)}\nEOF\nsleep 600\n`,
  );
  try {
    await sh(["tuistory", "launch", `sh ${runner}`, "-s", SESSION, "--cols", "104", "--rows", "3", ...envFlags]);
    await tui(["wait", "42%", "--timeout", "8000"]);
    await shot("status-line.png");
    await tui(["close"]);

    await sh([
      "tuistory", "launch", `${BINARY} config`, "-s", SESSION,
      "--cwd", home, "--cols", "110", "--rows", "32", "--timeout", "8000", ...envFlags,
    ]);
    await tui(["wait", "Segments", "--timeout", "8000"]);
    await shot("tui-home.png");

    await press("enter");
    await tui(["wait", "Every agent uses this list", "--timeout", "5000"]);
    await tui(["resize", "110", "40"]);
    await tui(["wait-idle", "--timeout", "3000"]);
    await shot("tui-segments.png");

    await press("esc", "down", "enter");
    await tui(["wait", "Available", "--timeout", "5000"]);
    await tui(["resize", "110", "32"]);
    await tui(["wait-idle", "--timeout", "3000"]);
    await shot("tui-agents.png");

    await press("esc", "down", "enter");
    await tui(["wait", "Separator", "--timeout", "5000"]);
    await tui(["resize", "110", "24"]);
    await tui(["wait-idle", "--timeout", "3000"]);
    await shot("tui-style.png");
  } finally {
    await tui(["close"], true);
  }
  return 0;
}

process.exit(await main());
