// Managed by statusmaxxx; `statusmaxxx uninstall amp` removes it.
// Status items are an experimental Amp plugin API and show in the CLI only.
import { execFile } from 'node:child_process'
import type { PluginAPI } from '@ampcode/plugin'

const BINARY = __STATUSMAXXX_BINARY__
const REFRESH_MS = 3000

type Rendered = { text: string; url: string | null }

function render(payload: object): Promise<Rendered> {
	return new Promise((resolve, reject) => {
		const child = execFile(BINARY, ['render', '--host', 'amp'], (error, stdout) =>
			error ? reject(error) : resolve(JSON.parse(stdout)),
		)
		child.stdin?.end(JSON.stringify(payload))
	})
}

export default function (amp: PluginAPI) {
	const item = amp.experimental?.createStatusItem()
	if (!item) return
	const root = amp.system.workspaceRoot
	const cwd = root ? amp.helpers.filePathFromURI(root) : process.cwd()
	let running = false

	const refresh = async () => {
		if (running) return
		running = true
		try {
			const { text, url } = await render({ cwd, session_id: amp.activeThread?.current?.id })
			item.update({ text, url: url ?? undefined })
		} catch (error) {
			item.update({ text: `statusmaxxx: ${error instanceof Error ? error.message : String(error)}` })
		} finally {
			running = false
		}
	}

	void refresh()
	amp.on('agent.end', () => void refresh())
	setInterval(refresh, REFRESH_MS)
}
