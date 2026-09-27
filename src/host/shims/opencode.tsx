/** @jsxImportSource @opentui/solid */
// Managed by statusmaxxx; `statusmaxxx uninstall opencode` removes it.
import { execFile } from "node:child_process"
import { createSignal } from "solid-js"
import type { TuiPlugin, TuiPluginModule } from "@opencode-ai/plugin/tui"

const BINARY = __STATUSMAXXX_BINARY__
const REFRESH_MS = 3000

type Rendered = { text: string; url: string | null }

function render(payload: object): Promise<Rendered> {
  return new Promise((resolve, reject) => {
    const child = execFile(BINARY, ["render", "--host", "opencode"], (error, stdout) =>
      error ? reject(error) : resolve(JSON.parse(stdout)),
    )
    child.stdin?.end(JSON.stringify(payload))
  })
}

const tui: TuiPlugin = async (api) => {
  const [text, setText] = createSignal("")
  let running = false

  const refresh = async () => {
    if (running) return
    running = true
    try {
      setText((await render({ cwd: api.state.path.directory })).text)
    } catch (error) {
      setText(`statusmaxxx: ${error instanceof Error ? error.message : String(error)}`)
    } finally {
      running = false
    }
  }

  void refresh()
  setInterval(refresh, REFRESH_MS)
  api.slots.register({
    order: 100,
    slots: {
      app_bottom() {
        return <text fg={api.theme.current.textMuted}>{text()}</text>
      },
    },
  })
}

const plugin: TuiPluginModule & { id: string } = { id: "statusmaxxx", tui }

export default plugin
