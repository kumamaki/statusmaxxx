// Managed by statusmaxxx; `statusmaxxx uninstall pi` removes it.
import { execFile } from "node:child_process";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

const BINARY = __STATUSMAXXX_BINARY__;
const STATUS_KEY = "statusmaxxx";
const REFRESH_MS = 3000;

type Rendered = { text: string; url: string | null };

function render(payload: object): Promise<Rendered> {
	return new Promise((resolve, reject) => {
		const child = execFile(BINARY, ["render", "--host", "pi"], (error, stdout) =>
			error ? reject(error) : resolve(JSON.parse(stdout)),
		);
		child.stdin?.end(JSON.stringify(payload));
	});
}

export default function (pi: ExtensionAPI) {
	let context: ExtensionContext | undefined;
	let timer: ReturnType<typeof setInterval> | undefined;
	let running = false;

	const refresh = async () => {
		if (!context || running) return;
		running = true;
		const ctx = context;
		try {
			const usage = ctx.getContextUsage();
			const { text } = await render({
				cwd: ctx.cwd,
				model: ctx.model ? { display_name: ctx.model.name } : undefined,
				context_window: usage?.percent == null ? undefined : { used_percentage: usage.percent },
			});
			ctx.ui.setStatus(STATUS_KEY, text || undefined);
		} catch (error) {
			ctx.ui.setStatus(STATUS_KEY, `statusmaxxx: ${error instanceof Error ? error.message : String(error)}`);
		} finally {
			running = false;
		}
	};

	pi.on("session_start", (_event, ctx) => {
		if (ctx.mode !== "tui") return;
		context = ctx;
		void refresh();
		timer ??= setInterval(refresh, REFRESH_MS);
	});
	pi.on("agent_end", (_event, ctx) => {
		context = ctx;
		void refresh();
	});
	pi.on("session_shutdown", () => {
		clearInterval(timer);
		timer = undefined;
		context = undefined;
	});
}
