/**
 * A `vesta-hubd --dev` for the end-to-end tests: either the one at
 * `VESTA_HUB_URL` (already running), or a fresh one started with
 * `cargo run` on a free port and stopped afterwards.
 */
import { spawn } from 'node:child_process'
import { createServer } from 'node:net'
import { fileURLToPath } from 'node:url'

const REPO_ROOT = fileURLToPath(new URL('../..', import.meta.url))

export interface RunningHub {
  /** e.g. `http://127.0.0.1:8080` */
  baseUrl: string
  stop: () => Promise<void>
}

export function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => {
        if (address !== null && typeof address === 'object') resolve(address.port)
        else reject(new Error('no port'))
      })
    })
  })
}

const sleep = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })

async function waitForHealth(baseUrl: string, timeoutMs: number, exited: () => string | null): Promise<void> {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const exit = exited()
    if (exit !== null) throw new Error(`vesta-hubd exited before it was ready:\n${exit}`)
    try {
      const response = await fetch(`${baseUrl}/api/v1/health`)
      if (response.ok) return
    } catch {
      // Not listening yet.
    }
    if (Date.now() > deadline) throw new Error(`vesta-hubd at ${baseUrl} was not ready after ${String(timeoutMs)} ms`)
    await sleep(250)
  }
}

export async function startHub(): Promise<RunningHub> {
  const external = process.env.VESTA_HUB_URL
  if (external !== undefined && external !== '') {
    const baseUrl = external.replace(/\/+$/, '')
    await waitForHealth(baseUrl, 10_000, () => null)
    return { baseUrl, stop: () => Promise.resolve() }
  }

  const port = await freePort()
  const baseUrl = `http://127.0.0.1:${String(port)}`
  // Own process group, so stopping it also stops cargo's child.
  const child = spawn('cargo', ['run', '--quiet', '-p', 'vesta-hubd', '--', '--dev', '--port', String(port)], {
    cwd: REPO_ROOT,
    detached: true,
    stdio: ['ignore', 'ignore', 'pipe'],
  })
  let stderr = ''
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk: string) => {
    stderr = (stderr + chunk).slice(-4_000)
  })
  let exitCode: number | null = null
  let exited = false
  const done = new Promise<void>((resolve) => {
    child.once('exit', (code) => {
      exited = true
      exitCode = code
      resolve()
    })
  })

  const signal = (name: NodeJS.Signals) => {
    if (exited || child.pid === undefined) return
    try {
      process.kill(-child.pid, name)
    } catch {
      // Already gone.
    }
  }
  const stop = async () => {
    signal('SIGINT')
    const killTimer = setTimeout(() => {
      signal('SIGKILL')
    }, 10_000)
    await done
    clearTimeout(killTimer)
  }

  try {
    // The first run may need to build the hub.
    await waitForHealth(baseUrl, 300_000, () => (exited ? `exit code ${String(exitCode)}\n${stderr}` : null))
  } catch (error) {
    await stop()
    throw error
  }
  return { baseUrl, stop }
}
