#!/usr/bin/env node
// Build the COMPLETE pinned OpenHamClock app into the private executable Tauri bundles.
// The upstream submodule stays byte-for-byte clean. Exact, fail-loud adaptations are
// applied to a temporary copy for loopback confinement, framing, packaged geo data and
// the audited Nexus data/control bridge.

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = join(repo, 'third_party', 'openhamclock')
const integration = join(repo, 'integrations', 'openhamclock')
const manifest = JSON.parse(readFileSync(join(integration, 'upstream.json'), 'utf8'))
const stage = join(repo, 'src-tauri', 'resources', 'openhamclock')

function run(command, args, cwd = repo) {
  execFileSync(command, args, { cwd, stdio: 'inherit' })
}

function replaceOnce(path, before, after) {
  const input = readFileSync(path, 'utf8')
  const first = input.indexOf(before)
  if (first < 0 || input.indexOf(before, first + before.length) >= 0) {
    throw new Error(`OpenHamClock integration anchor changed: ${path}\nExpected exactly one:\n${before}`)
  }
  writeFileSync(path, input.replace(before, after))
}

function thirdPartyNotices(roots) {
  const seen = new Set()
  const sections = []
  for (const root of roots) {
    const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'))
    for (const relative of Object.keys(lock.packages || {}).filter((name) => name.startsWith('node_modules/')).sort()) {
      const dir = join(root, relative)
      const packageJson = join(dir, 'package.json')
      if (!existsSync(packageJson)) continue
      const pkg = JSON.parse(readFileSync(packageJson, 'utf8'))
      const identity = `${pkg.name || relative}@${pkg.version || 'unknown'}`
      if (seen.has(identity)) continue
      seen.add(identity)
      const licenseFiles = readdirSync(dir)
        .filter((name) => /^(licen[cs]e|copying|notice)(\.|$)/i.test(name))
        .sort()
      const texts = licenseFiles.map((name) => readFileSync(join(dir, name), 'utf8').trim()).filter(Boolean)
      sections.push(`--- ${identity} — ${pkg.license || 'license in accompanying text'} ---\n${texts.join('\n\n')}`)
    }
  }
  return `OpenHamClock third-party notices\nGenerated from pinned package locks.\n\n${sections.join('\n\n')}\n`
}

function filesUnder(root) {
  const out = []
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name)
      if (entry.isDirectory()) visit(path)
      else if (entry.isFile()) out.push(path)
    }
  }
  visit(root)
  return out.sort()
}

function parityInventory(work) {
  const panelSource = readFileSync(join(work, 'src', 'panelDefs.js'), 'utf8')
  const panels = [...panelSource.matchAll(/(?:'([^']+)'|\b([a-z][a-z0-9-]*)):\s*\{\s*name:\s*'([^']+)'/g)]
    .map((match) => ({ id: match[1] || match[2], name: match[3], source: 'src/panelDefs.js' }))
  const settingsSource = readFileSync(join(work, 'src', 'components', 'SettingsPanel.jsx'), 'utf8')
  const tabBlock = settingsSource.match(/const SETTINGS_TABS = \[([\s\S]*?)\];/)?.[1] || ''
  const settingsTabs = [...tabBlock.matchAll(/'([^']+)'/g)].map((match) => match[1])
  const routeFiles = [join(work, 'server.js'), ...filesUnder(join(work, 'server', 'routes')).filter((path) => path.endsWith('.js') && !path.endsWith('.test.js'))]
  const routes = []
  for (const path of routeFiles) {
    const source = readFileSync(path, 'utf8')
    const relative = path.slice(work.length + 1)
    for (const match of source.matchAll(/app\.(get|post|put|patch|delete|options)\(\s*(['"])(\/[^'"]+)\2/g)) {
      const line = source.slice(0, match.index).split('\n').length
      routes.push({ method: match[1].toUpperCase(), path: match[3], source: `${relative}:${line}` })
    }
  }
  const storage = new Map()
  for (const path of filesUnder(join(work, 'src')).filter((path) => /\.(js|jsx)$/.test(path))) {
    const source = readFileSync(path, 'utf8')
    for (const match of source.matchAll(/\b(localStorage|sessionStorage)\.(?:getItem|setItem|removeItem)\(\s*['"]([^'"]+)['"]/g)) {
      storage.set(`${match[1]}:${match[2]}`, { kind: match[1], key: match[2], source: path.slice(work.length + 1) })
    }
  }
  const configuration = new Map()
  for (const path of [join(work, 'server.js'), ...filesUnder(join(work, 'server')).filter((file) => file.endsWith('.js') && !file.endsWith('.test.js'))]) {
    const source = readFileSync(path, 'utf8')
    for (const match of source.matchAll(/process\.env\.([A-Z][A-Z0-9_]*)/g)) {
      const key = match[1]
      if (!configuration.has(key)) configuration.set(key, { key, source: path.slice(work.length + 1) })
    }
  }
  const distFiles = filesUnder(join(work, 'dist')).map((path) => {
    const bytes = readFileSync(path)
    return { path: path.slice(join(work, 'dist').length + 1), size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }
  })
  return { panels, settingsTabs, routes, storage: [...storage.values()], configuration: [...configuration.values()].sort((a, b) => a.key.localeCompare(b.key)), distFiles }
}

function targetName() {
  const at = process.argv.indexOf('--target')
  if (at >= 0 && process.argv[at + 1]) return process.argv[at + 1]
  if (process.platform === 'win32') return 'win-x64'
  if (process.platform === 'darwin') return process.arch === 'arm64' ? 'macos-arm64' : 'macos-x64'
  return process.arch === 'arm64' ? 'linux-arm64' : 'linux-x64'
}

const targets = new Set(['win-x64', 'macos-arm64', 'macos-x64', 'linux-x64', 'linux-arm64'])
const target = targetName()
if (!targets.has(target)) throw new Error(`Unsupported OpenHamClock target: ${target}`)
if (!existsSync(join(source, 'package.json'))) {
  throw new Error('OpenHamClock submodule is absent; run git submodule update --init third_party/openhamclock')
}
const actual = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
if (actual !== manifest.commit) {
  throw new Error(`OpenHamClock pin mismatch: expected ${manifest.commit}, found ${actual}`)
}

const temp = mkdtempSync(join(tmpdir(), 'rd-nexus-openhamclock-'))
const work = join(temp, 'openhamclock')
try {
  cpSync(source, work, {
    recursive: true,
    filter: (path) => basename(path) !== '.git' && basename(path) !== 'node_modules' && basename(path) !== 'dist',
  })
  replaceOnce(
    join(work, 'server.js'),
    "app.listen(PORT, '0.0.0.0', () => {",
    'const nexusServer = app.listen(PORT, HOST, () => {\n' +
      "  const boundPort = nexusServer.address().port;\n" +
      "  if (process.env.OHC_NEXUS_PORT_FILE) fs.writeFileSync(process.env.OHC_NEXUS_PORT_FILE, JSON.stringify({ port: boundPort }));",
  )
  replaceOnce(
    join(work, 'server.js'),
    '// ── Start server ──\nconst nexusServer = app.listen(PORT, HOST, () => {',
    "// ── Nexus-owned process lifetime ──\n" +
      "const nexusParentPid = Number(process.env.OHC_NEXUS_PARENT_PID);\n" +
      "if (Number.isInteger(nexusParentPid) && nexusParentPid > 0) {\n" +
      "  setInterval(() => {\n" +
      "    try { process.kill(nexusParentPid, 0); } catch { process.exit(0); }\n" +
      "  }, 2000).unref();\n" +
      "}\n\n" +
      '// ── Start server ──\nconst nexusServer = app.listen(PORT, HOST, () => {',
  )
  replaceOnce(
    join(work, 'server.js'),
    "});\n\nfunction openInBrowser(url) {",
    "});\n" +
      "nexusServer.on('error', (error) => {\n" +
      "  if (error.code === 'EADDRINUSE' && process.env.OHC_NEXUS_EMBED === '1') {\n" +
      "    nexusServer.listen(0, HOST);\n" +
      "    return;\n" +
      "  }\n" +
      "  throw error;\n" +
      "});\n\nfunction openInBrowser(url) {",
  )
  replaceOnce(
    join(work, 'server', 'middleware', 'index.js'),
    '      crossOriginEmbedderPolicy: false, // Required for cross-origin tile loading\n',
    "      crossOriginEmbedderPolicy: false, // Required for cross-origin tile loading\n" +
      "      frameguard: process.env.OHC_NEXUS_EMBED === '1' ? false : undefined,\n",
  )
  replaceOnce(
    join(work, 'server', 'utils', 'geoTz.js'),
    "const { find } = require('geo-tz/all');",
    "const { find } = require('geo-tz/dist/find-all');",
  )
  replaceOnce(
    join(work, 'server', 'config.js'),
    '        process.env[key] = value.trim();',
    '        if (process.env[key] === undefined) process.env[key] = value.trim();',
  )
  replaceOnce(
    join(work, 'public', 'sw-policy.js'),
    "var STATIC_ASSET_PREFIXES = ['/assets/', '/icons/', '/geo/', '/models/', '/wasm/'];",
    "var STATIC_ASSET_PREFIXES = ['/assets/', '/vendor/', '/icons/', '/geo/', '/models/', '/wasm/'];",
  )
  replaceOnce(
    join(work, 'public', 'sw.js'),
    "var assetUrls = html.match(/\\/assets\\/[^\"'\\s>)]+/g) || [];",
    "var assetUrls = [];\n          var localRef = /(?:src|href)=[\"'](\\/[^\"'#\\s>]+)[\"']/g;\n          var match;\n          while ((match = localRef.exec(html)) !== null) assetUrls.push(match[1]);",
  )
  cpSync(join(integration, 'nexusBridge.js'), join(work, 'src', 'nexusBridge.js'))
  cpSync(join(integration, 'nexusLogMerge.mjs'), join(work, 'src', 'nexusLogMerge.js'))
  cpSync(join(integration, 'nexusMirrorStore.mjs'), join(work, 'src', 'nexusMirrorStore.js'))
  replaceOnce(
    join(work, 'src', 'main.jsx'),
    "import { setupServiceWorker } from './pwa/registerServiceWorker';\n",
    "import { setupServiceWorker } from './pwa/registerServiceWorker';\nimport './nexusBridge.js';\n",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "const DB_NAME = 'openhamclock-logbook';\nconst DB_VERSION = 1;",
    "import { reconcileNexusLog } from '../nexusLogMerge.js';\nimport { commitNexusDelta, commitNexusSnapshot, loadNexusMirror } from '../nexusMirrorStore.js';\n\nconst DB_NAME = 'openhamclock-logbook';\nconst DB_VERSION = 2;",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "          store.createIndex('qso_date', 'qso_date', { unique: false });\n        }",
    "          store.createIndex('qso_date', 'qso_date', { unique: false });\n        }\n        if (!database.objectStoreNames.contains('nexus-mirror')) {\n          database.createObjectStore('nexus-mirror', { keyPath: 'key' });\n        }",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "  panelMounts: 0, // number of currently mounted LogbookPanel instances\n",
    "  panelMounts: 0, // number of currently mounted LogbookPanel instances\n  nexusApplying: false,\n  nexusIds: new Set(),\n  nexusRevision: 0,\n",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "/** Add one QSO. Assigns an id when missing. Returns the stored record. */\n",
    "const NEXUS_LEDGER_KEY = 'openhamclock_nexus_mirror_ids_v1';\nconst notifyNexus = (action, payload) => {\n  if (window.__NEXUS_EMBEDDED__ && !state.nexusApplying) {\n    window.parent.postMessage({ source: 'openhamclock', type: `logbook:${action}`, payload }, '*');\n  }\n};\n\n/** Commit a cold Nexus snapshot and its binding ledger in one IndexedDB transaction. */\nexport const mergeFromNexus = async (records, revision = 0) => {\n  await init();\n  state.nexusApplying = true;\n  try {\n    const durable = await loadNexusMirror();\n    let previousIds = new Set(durable.ids);\n    if (previousIds.size === 0) {\n      try { previousIds = new Set(JSON.parse(localStorage.getItem(NEXUS_LEDGER_KEY) || '[]')); } catch {}\n    }\n    const next = reconcileNexusLog(state.qsos, previousIds, records);\n    await commitNexusSnapshot(next.records, next.ids, revision);\n    state.qsos = next.records;\n    state.nexusIds = new Set(next.ids);\n    state.nexusRevision = revision;\n    try { localStorage.removeItem(NEXUS_LEDGER_KEY); } catch {}\n    notify();\n  } finally {\n    state.nexusApplying = false;\n  }\n};\n\n/** Apply one compact Nexus mutation transaction without rebuilding the mirror. */\nexport const applyNexusDelta = async ({ upserts = [], deleted = [], revision = 0 }) => {\n  await init();\n  state.nexusApplying = true;\n  try {\n    const byId = new Map(state.qsos.map((record) => [record.id, record]));\n    for (const id of deleted) byId.delete(id);\n    for (const record of upserts) byId.set(record.id, record);\n    await commitNexusDelta(upserts, deleted, revision);\n    for (const id of deleted) state.nexusIds.delete(id);\n    for (const record of upserts) state.nexusIds.add(record.id);\n    state.nexusRevision = revision;\n    state.qsos = [...byId.values()];\n    notify();\n  } finally {\n    state.nexusApplying = false;\n  }\n};\n\n/** Add one QSO. Assigns an id when missing. Returns the stored record. */\n",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "    await commitNexusSnapshot(next.records, next.ids, revision);\n    state.qsos = next.records;",
    "    const commit = await commitNexusSnapshot(next.records, next.ids, revision);\n    if (commit.status !== 'committed') return commit;\n    state.qsos = next.records;",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "    notify();\n  } finally {\n    state.nexusApplying = false;\n  }\n};\n\n/** Apply one compact Nexus mutation transaction without rebuilding the mirror. */",
    "    notify();\n    return commit;\n  } finally {\n    state.nexusApplying = false;\n  }\n};\n\n/** Apply one compact Nexus mutation transaction without rebuilding the mirror. */",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "export const applyNexusDelta = async ({ upserts = [], deleted = [], revision = 0 }) => {",
    "export const applyNexusDelta = async ({ upserts = [], deleted = [], baseRevision = 0, revision = 0 }) => {",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "    await commitNexusDelta(upserts, deleted, revision);\n    for (const id of deleted) state.nexusIds.delete(id);",
    "    const commit = await commitNexusDelta(upserts, deleted, baseRevision, revision);\n    if (commit.status !== 'committed') return commit;\n    for (const id of deleted) state.nexusIds.delete(id);",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "    state.qsos = [...byId.values()];\n    notify();\n  } finally {",
    "    state.qsos = [...byId.values()];\n    notify();\n    return commit;\n  } finally {",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "  return record;\n};\n\n/**\n * Bulk add (ADIF import).",
    "  notifyNexus('add', record);\n  return record;\n};\n\n/**\n * Bulk add (ADIF import).",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "  return { imported: fresh.length, skipped };\n};\n\n/** Update a QSO",
    "  if (fresh.length > 0) notifyNexus('addMany', fresh);\n  return { imported: fresh.length, skipped };\n};\n\n/** Update a QSO",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "  return record;\n};\n\n/** Remove a QSO by id.",
    "  notifyNexus('update', record);\n  return record;\n};\n\n/** Remove a QSO by id.",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "  return true;\n};\n\n/** Delete every QSO. */",
    "  notifyNexus('remove', { id });\n  return true;\n};\n\n/** Delete every QSO. */",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "    persistError('clear', err);\n  }\n};\n\n/**\n * Subscribe to log changes.",
    "    persistError('clear', err);\n  }\n  notifyNexus('clear', {});\n};\n\n/**\n * Subscribe to log changes.",
  )
  replaceOnce(
    join(work, 'src', 'services', 'logbookStore.js'),
    "  count,\n  subscribe,\n",
    "  count,\n  subscribe,\n  mergeFromNexus,\n  applyNexusDelta,\n",
  )

  const rigContext = join(work, 'src', 'contexts', 'RigContext.jsx')
  replaceOnce(
    rigContext,
    "export const RigProvider = ({ children, rigConfig }) => {\n",
    "export const RigProvider = ({ children, rigConfig }) => {\n  const nexusEmbedded = window.__NEXUS_EMBEDDED__ === true;\n",
  )
  replaceOnce(
    rigContext,
    "  useEffect(() => {\n    if (rigConfig && !rigConfig.enabled) {",
    "  useEffect(() => {\n    if (!nexusEmbedded) return;\n    const onState = (event) => {\n      const data = event.detail || {};\n      setRigState((prev) => ({ ...prev, connected: data.connected !== false, freq: data.freq || prev.freq, mode: data.mode || prev.mode, ptt: data.ptt ?? prev.ptt, lastUpdate: Date.now() }));\n      setError(null);\n    };\n    window.addEventListener('nexus-rig-state', onState);\n    return () => window.removeEventListener('nexus-rig-state', onState);\n  }, [nexusEmbedded]);\n\n  useEffect(() => {\n    if (rigConfig && !rigConfig.enabled) {",
  )
  for (const [anchor, action, optimistic] of [
    ["    async (freq) => {\n      if (!rigConfig?.enabled) return;", 'setFreq', "\n      if (nexusEmbedded) { setRigState((prev) => ({ ...prev, freq, lastUpdate: Date.now() })); window.parent.postMessage({ source: 'openhamclock', type: 'rig:setFreq', payload: { freq } }, '*'); return; }"],
    ["    async (mode) => {\n      if (!rigConfig?.enabled) return;", 'setMode', "\n      if (nexusEmbedded) { setRigState((prev) => ({ ...prev, mode, lastUpdate: Date.now() })); window.parent.postMessage({ source: 'openhamclock', type: 'rig:setMode', payload: { mode } }, '*'); return; }"],
    ["    async (enabled) => {\n      if (!rigConfig?.enabled) return;", 'setPTT', "\n      if (nexusEmbedded) { setRigState((prev) => ({ ...prev, ptt: enabled, lastUpdate: Date.now() })); window.parent.postMessage({ source: 'openhamclock', type: 'rig:setPTT', payload: { ptt: enabled } }, '*'); return; }"],
  ]) {
    replaceOnce(rigContext, anchor, anchor.replace('if (!rigConfig?.enabled) return;', 'if (!rigConfig?.enabled && !nexusEmbedded) return;') + optimistic)
  }
  replaceOnce(
    rigContext,
    "    enabled: rigConfig?.enabled,\n",
    "    enabled: nexusEmbedded || rigConfig?.enabled,\n",
  )

  run('npm', ['ci', '--ignore-scripts'], work)
  run('npm', ['run', 'build'], work)
  const inventory = parityInventory(work)
  run('npm', ['ci', '--ignore-scripts'], integration)

  mkdirSync(stage, { recursive: true })
  // The resource directory is build input, never OpenHamClock's writable home.
  // Remove any state left by an accidental direct launch before bundling it.
  for (const name of ['.env', 'config.json', 'data', 'openhamclock.log', 'cluster-status.json']) {
    rmSync(join(stage, name), { recursive: true, force: true })
  }
  const executable = join(stage, target.startsWith('win-') ? 'openhamclock.exe' : 'openhamclock')
  rmSync(join(stage, target.startsWith('win-') ? 'openhamclock' : 'openhamclock.exe'), { force: true })
  rmSync(executable, { force: true })
  const pkg = join(integration, 'node_modules', '@yao-pkg', 'pkg', 'lib-es5', 'bin.js')
  run(process.execPath, [
    pkg, 'server.js', '--target', `node22-${target}`,
    '--output', executable, '--no-bytecode', '--public', '--public-packages', '*',
  ], work)
  if (!target.startsWith('win-')) run('chmod', ['755', executable])
  cpSync(join(source, 'LICENSE'), join(stage, 'OPENHAMCLOCK-LICENSE.txt'))
  writeFileSync(join(stage, 'parity-inventory.json'), JSON.stringify(inventory, null, 2) + '\n')
  writeFileSync(
    join(stage, 'OPENHAMCLOCK-THIRD-PARTY-NOTICES.txt'),
    thirdPartyNotices([work, integration]),
  )
  const nodeLicenseResponse = await fetch(manifest.nodeRuntime.licenseUrl)
  if (!nodeLicenseResponse.ok) throw new Error(`download Node.js runtime license: HTTP ${nodeLicenseResponse.status}`)
  const nodeLicense = Buffer.from(await nodeLicenseResponse.arrayBuffer())
  const nodeLicenseSha = createHash('sha256').update(nodeLicense).digest('hex')
  if (nodeLicenseSha !== manifest.nodeRuntime.licenseSha256) {
    throw new Error(`Node.js runtime license checksum mismatch: ${nodeLicenseSha}`)
  }
  writeFileSync(join(stage, 'NODE-RUNTIME-LICENSE.txt'), nodeLicense)
  const executableSha256 = createHash('sha256').update(readFileSync(executable)).digest('hex')
  writeFileSync(join(stage, 'provenance.json'), JSON.stringify({
    ...manifest,
    target,
    executable: basename(executable),
    executableSha256,
  }, null, 2) + '\n')
  process.stdout.write(`Staged OpenHamClock ${manifest.version} (${actual}) for ${target}\n`)
} finally {
  rmSync(temp, { recursive: true, force: true })
}
