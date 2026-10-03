#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const runtime = mkdtempSync(join(tmpdir(), 'neural-dx-consumer-'))
const ready = join(runtime, 'ready.json')
const executableArg = process.argv.indexOf('--executable')
const executable = executableArg >= 0
  ? resolve(process.argv[executableArg + 1])
  : join(repo, 'src-tauri', 'resources', 'neural-dx', process.platform === 'win32' ? 'neural-dx-watcher.exe' : 'neural-dx-watcher')
const child = spawn(executable, [], {
  cwd: runtime,
  stdio: 'ignore',
  env: {
    ...process.env,
    NEURAL_DX_PORT: '0',
    NEURAL_DX_CALLSIGN: 'N0CALL',
    NEURAL_DX_GRID: 'FN31',
    NEURAL_DX_NEXUS_EMBED: '1',
    NEURAL_DX_NEXUS_TOKEN: 'consumer-test-token',
    NEURAL_DX_READY_FILE: ready,
    NEURAL_DX_NEXUS_PARENT_PID: String(process.pid),
  },
})

const pause = (ms) => new Promise((resolvePromise) => setTimeout(resolvePromise, ms))
try {
  for (let attempt = 0; attempt < 300 && !existsSync(ready); attempt += 1) await pause(100)
  if (!existsSync(ready)) throw new Error('staged Neural DX sidecar did not become ready')
  const { port } = JSON.parse(readFileSync(ready, 'utf8'))
  const headers = { 'X-Nexus-Token': 'consumer-test-token', 'Content-Type': 'application/json' }
  const sync = await fetch(`http://127.0.0.1:${port}/api/nexus/sync`, {
    method: 'POST', headers, body: JSON.stringify({
      spots: [],
      qsoSummary: {
        confirmedCalls: ['K2SUM'], confirmedDxcc: [], workedCalls: ['K2SUM'], workedDxcc: [],
        dxccByBand: {}, totalQso: 1, totalConfirmed: 1,
      },
    }),
  })
  if (!sync.ok) throw new Error(`native sync returned HTTP ${sync.status}`)
  const consumer = await fetch(`http://127.0.0.1:${port}/api/lotw/check_call?call=K2SUM`)
  const result = await consumer.json()
  if (!consumer.ok || result.call_confirmed !== true || result.logged_in !== false) {
    throw new Error(`Nexus knowledge did not reach the logged-out consumer: ${JSON.stringify(result)}`)
  }
  process.stdout.write('Neural DX Nexus knowledge consumer passed with LoTW logged out\n')

  // restart_neural_dx terminates the exact launcher while Nexus remains alive. PyInstaller's
  // onefile worker must follow that launcher instead of surviving as a second local server.
  child.kill()
  let launcherWorkerStopped = false
  for (let attempt = 0; attempt < 100; attempt += 1) {
    await pause(100)
    try {
      await fetch(`http://127.0.0.1:${port}/api/nexus/status`, { headers })
    } catch {
      launcherWorkerStopped = true
      break
    }
  }
  if (!launcherWorkerStopped) throw new Error('Neural DX worker outlived its launcher during restart')
  process.stdout.write('Neural DX restart shutdown passed\n')

  const owner = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' })
  const ownedRuntime = mkdtempSync(join(tmpdir(), 'neural-dx-parent-lifetime-'))
  const ownedReady = join(ownedRuntime, 'ready.json')
  const owned = spawn(executable, [], {
    cwd: ownedRuntime,
    stdio: 'ignore',
    env: {
      ...process.env,
      NEURAL_DX_PORT: '0',
      NEURAL_DX_CALLSIGN: 'N0CALL',
      NEURAL_DX_GRID: 'FN31',
      NEURAL_DX_NEXUS_EMBED: '1',
      NEURAL_DX_NEXUS_TOKEN: 'parent-lifetime-test-token',
      NEURAL_DX_READY_FILE: ownedReady,
      NEURAL_DX_NEXUS_PARENT_PID: String(owner.pid),
    },
  })
  try {
    for (let attempt = 0; attempt < 300 && !existsSync(ownedReady); attempt += 1) await pause(100)
    if (!existsSync(ownedReady)) throw new Error('parent-owned Neural DX sidecar did not become ready')
    owner.kill()
    for (let attempt = 0; attempt < 100 && owned.exitCode === null; attempt += 1) await pause(100)
    if (owned.exitCode === null) throw new Error('Neural DX sidecar outlived its owner process')
    process.stdout.write('Neural DX owner-lifetime shutdown passed\n')
  } finally {
    owner.kill()
    owned.kill()
    rmSync(ownedRuntime, { recursive: true, force: true })
  }
} finally {
  child.kill()
  rmSync(runtime, { recursive: true, force: true })
}
