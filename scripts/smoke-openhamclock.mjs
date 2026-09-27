#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'node:net'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const resourceDir = join(root, 'src-tauri', 'resources', 'openhamclock')
const executable = join(resourceDir, process.platform === 'win32' ? 'openhamclock.exe' : 'openhamclock')
const inventory = JSON.parse(readFileSync(join(resourceDir, 'parity-inventory.json'), 'utf8'))
if (!existsSync(executable)) throw new Error('OpenHamClock executable is not staged')

const occupiedPort = () => new Promise((resolvePort, reject) => {
  const server = createServer()
  server.once('error', reject)
  server.listen(0, '127.0.0.1', () => {
    const address = server.address()
    resolvePort({ server, port: address.port })
  })
})
const wait = (ms) => new Promise((resolveWait) => setTimeout(resolveWait, ms))
const children = new Set()

function start(port, home, extra = {}) {
  const portFile = join(home, `smoke-${Date.now()}-${Math.random().toString(16).slice(2)}.port.json`)
  const child = spawn(executable, [], {
    env: {
      ...process.env,
      HOST: '127.0.0.1', PORT: String(port), OPENHAMCLOCK_HOME: home,
      OHC_NEXUS_EMBED: '1', AUTO_UPDATE_ENABLED: 'false', OPEN_BROWSER: 'false',
      OHC_NEXUS_PORT_FILE: portFile,
      SETTINGS_SYNC: 'true', WSJTX_ENABLED: 'false', N1MM_UDP_ENABLED: 'false',
      CALLSIGN: 'N0CALL', LOCATOR: 'AA00aa', ...extra,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  child.diagnostics = ''
  child.portFile = portFile
  child.stdout.on('data', (chunk) => { child.diagnostics += chunk.toString() })
  child.stderr.on('data', (chunk) => { child.diagnostics += chunk.toString() })
  children.add(child)
  child.once('exit', () => children.delete(child))
  return child
}

async function ready(child, timeout = 45_000) {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    if (child.exitCode != null || child.signalCode != null) throw new Error(`OpenHamClock exited early (${child.exitCode ?? child.signalCode})\n${child.diagnostics}`)
    try {
      const { port } = JSON.parse(readFileSync(child.portFile, 'utf8'))
      const response = await fetch(`http://127.0.0.1:${port}/api/health?format=json`)
      const body = await response.json()
      if (response.ok && body.status === 'ok' && body.version === '26.8.0') return { body, port }
    } catch {}
    await wait(150)
  }
  throw new Error(`OpenHamClock did not become healthy in ${timeout}ms\n${child.diagnostics}`)
}

async function waitExit(child, timeout = 10_000) {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    if (child.exitCode != null || child.signalCode != null) return child.exitCode ?? child.signalCode
    await wait(100)
  }
  throw new Error(`process did not exit in ${timeout}ms\n${child.diagnostics}`)
}

function routePath(path) {
  const values = { type: 'sdo', callsign: 'N0CALL', identifier: 'N0CALL', reference: 'K-0001', code: 'TEST01', file: 'index.json' }
  return path.replace(/:([A-Za-z0-9_]+)/g, (_, name) => values[name] || 'test')
}

const homes = [mkdtempSync(join(tmpdir(), 'rd-nexus-openhamclock-smoke-'))]
try {
  let child = start(0, homes[0])
  const { port } = await ready(child)
  const base = `http://127.0.0.1:${port}`

  const page = await fetch(`${base}/?nexus=1`)
  const csp = page.headers.get('content-security-policy') || ''
  if (!page.ok || page.headers.has('x-frame-options') || /frame-ancestors\s+'none'/.test(csp)) {
    throw new Error('embedded page is not frameable')
  }
  const geo = await fetch(`${base}/api/geo-time?grid=FN31pr`)
  const geoBody = await geo.json()
  if (!geo.ok || !geoBody.timezone) throw new Error('packaged geo-tz data is unavailable')

  let javascript = ''
  for (const asset of inventory.distFiles) {
    const response = await fetch(`${base}/${asset.path}`)
    if (!response.ok) throw new Error(`packaged asset missing: ${asset.path} (${response.status})`)
    const bytes = Buffer.from(await response.arrayBuffer())
    const sha = createHash('sha256').update(bytes).digest('hex')
    if (sha !== asset.sha256) throw new Error(`packaged asset changed: ${asset.path}`)
    if (asset.path.endsWith('.js')) javascript += bytes.toString('utf8')
  }
  if (!javascript.includes('/sw.js?v=') || !/26\.8\.0-[a-z0-9]+/.test(javascript)) {
    throw new Error('service-worker registration is not stamped with a release-specific cache version')
  }
  const sw = await fetch(`${base}/sw.js`).then((res) => res.text())
  const swPolicy = await fetch(`${base}/sw-policy.js`).then((res) => res.text())
  if (!sw.includes('staleCacheNames') || !sw.includes('caches.delete') || !swPolicy.includes('staleCacheNames')) {
    throw new Error('service-worker upgrade path does not evict stale OpenHamClock caches')
  }
  for (const route of inventory.routes) {
    if (route.path.includes('*')) continue
    const response = await fetch(`${base}${routePath(route.path)}`, { method: 'OPTIONS' })
    if (response.status === 404) throw new Error(`packaged route not registered: ${route.method} ${route.path}`)
  }

  const marker = { openhamclock_parity_probe: `persist-${Date.now()}` }
  const saved = await fetch(`${base}/api/settings`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(marker),
  })
  if (!saved.ok) throw new Error(`settings persistence write failed: ${saved.status}`)

  const occupied = await occupiedPort()
  const fallbackHomes = [
    mkdtempSync(join(tmpdir(), 'rd-nexus-openhamclock-fallback-a-')),
    mkdtempSync(join(tmpdir(), 'rd-nexus-openhamclock-fallback-b-')),
  ]
  homes.push(...fallbackHomes)
  const fallbackA = start(occupied.port, fallbackHomes[0])
  const fallbackB = start(occupied.port, fallbackHomes[1])
  const [readyA, readyB] = await Promise.all([ready(fallbackA), ready(fallbackB)])
  if (readyA.port === occupied.port || readyB.port === occupied.port || readyA.port === readyB.port) {
    throw new Error('bind fallback did not allocate distinct loopback ports')
  }
  fallbackA.kill(); fallbackB.kill()
  await Promise.all([waitExit(fallbackA), waitExit(fallbackB)])
  await new Promise((resolveClose) => occupied.server.close(resolveClose))

  child.kill()
  await waitExit(child)
  child = start(0, homes[0])
  const restarted = await ready(child)
  const restored = await fetch(`http://127.0.0.1:${restarted.port}/api/settings`).then((res) => res.json())
  if (restored.openhamclock_parity_probe !== marker.openhamclock_parity_probe) {
    throw new Error('settings did not persist across packaged runtime restart')
  }
  child.kill()
  await waitExit(child)

  const parent = spawn(process.execPath, ['-e', 'setTimeout(() => process.exit(0), 300)'])
  const parentHome = mkdtempSync(join(tmpdir(), 'rd-nexus-openhamclock-parent-'))
  homes.push(parentHome)
  const owned = start(0, parentHome, { OHC_NEXUS_PARENT_PID: String(parent.pid) })
  await ready(owned)
  await waitExit(parent, 5_000)
  await waitExit(owned, 15_000)

  process.stdout.write(`OpenHamClock packaged runtime parity smoke passes: ${inventory.routes.length} routes, ${inventory.distFiles.length} assets, persistence, collision, duplicate-instance, restart, and parent cleanup.\n`)
} finally {
  for (const child of children) child.kill()
  for (const home of homes.filter(Boolean)) rmSync(home, { recursive: true, force: true })
}
