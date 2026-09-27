#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'node:net'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const executable = join(root, 'src-tauri', 'resources', 'openhamclock', process.platform === 'win32' ? 'openhamclock.exe' : 'openhamclock')
if (!existsSync(executable)) throw new Error('OpenHamClock executable is not staged')

const port = await new Promise((resolvePort, reject) => {
  const server = createServer()
  server.once('error', reject)
  server.listen(0, '127.0.0.1', () => {
    const address = server.address()
    server.close(() => resolvePort(address.port))
  })
})
const home = mkdtempSync(join(tmpdir(), 'rd-nexus-openhamclock-smoke-'))
const child = spawn(executable, [], {
  env: {
    ...process.env,
    HOST: '127.0.0.1',
    PORT: String(port),
    OPENHAMCLOCK_HOME: home,
    OHC_NEXUS_EMBED: '1',
    AUTO_UPDATE_ENABLED: 'false',
    WSJTX_ENABLED: 'false',
    N1MM_UDP_ENABLED: 'false',
    CALLSIGN: 'N0CALL',
    LOCATOR: 'AA00aa',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})
let diagnostics = ''
child.stdout.on('data', (chunk) => { diagnostics += chunk.toString() })
child.stderr.on('data', (chunk) => { diagnostics += chunk.toString() })

try {
  const deadline = Date.now() + 45_000
  let response
  while (Date.now() < deadline) {
    if (child.exitCode != null) throw new Error('OpenHamClock exited early (' + child.exitCode + ')\n' + diagnostics)
    try {
      response = await fetch('http://127.0.0.1:' + port + '/api/health')
      if (response.ok) break
    } catch {}
    await new Promise((resolveWait) => setTimeout(resolveWait, 200))
  }
  if (!response?.ok) throw new Error('OpenHamClock did not become ready in 45 seconds\n' + diagnostics)
  const page = await fetch('http://127.0.0.1:' + port + '/?nexus=1')
  if (!page.ok || page.headers.has('x-frame-options')) throw new Error('embedded page is not frameable')
  const geo = await fetch('http://127.0.0.1:' + port + '/api/geo-time?grid=FN31pr')
  const geoBody = await geo.json()
  if (!geo.ok || !geoBody.timezone) throw new Error('packaged geo-tz data is unavailable')
  process.stdout.write('OpenHamClock packaged runtime, embedding headers, and geo data pass.\n')
} finally {
  child.kill()
  rmSync(home, { recursive: true, force: true })
}
