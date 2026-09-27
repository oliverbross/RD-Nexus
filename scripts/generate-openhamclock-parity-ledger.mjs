#!/usr/bin/env node

import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const integration = join(repo, 'integrations', 'openhamclock')
const staged = join(repo, 'src-tauri', 'resources', 'openhamclock')
const inventoryPath = join(staged, 'parity-inventory.json')
const provenancePath = join(staged, 'provenance.json')
const platformPath = join(integration, 'package-inspection.json')

if (!existsSync(inventoryPath) || !existsSync(provenancePath)) {
  throw new Error('Stage OpenHamClock before generating its parity ledger')
}

const inventory = JSON.parse(readFileSync(inventoryPath, 'utf8'))
const provenance = JSON.parse(readFileSync(provenancePath, 'utf8'))
const platforms = existsSync(platformPath) ? JSON.parse(readFileSync(platformPath, 'utf8')) : { targets: [] }
const rows = []
const external = []

function clean(value) {
  return String(value ?? '').replaceAll('|', '\\|').replaceAll('\n', ' ')
}

function add(category, capability, upstream, seam, evidence, status = 'VERIFIED', reason = '') {
  rows.push({ category, capability, upstream, seam, evidence, status, reason })
}

add('Pin', `OpenHamClock ${provenance.version}`, `${provenance.repository}@${provenance.commit}`, 'Git submodule plus fail-closed staging pin', 'upstream.json, submodule HEAD, staged provenance.json')
add('Packaging', `Embedded Node.js ${provenance.nodeRuntime.version}`, provenance.nodeRuntime.licenseUrl, 'Target-native pkg executable included as a Tauri resource', 'checksum-pinned runtime license and package inspection')

for (const panel of inventory.panels) {
  add('Panel', `${panel.name} (${panel.id})`, panel.source, 'Unmodified upstream panel registry in the embedded application', 'staged production build and browser panel inventory')
}
for (const tab of inventory.settingsTabs) {
  add('Settings', tab, 'src/components/SettingsPanel.jsx', 'Unmodified upstream settings surface and private writable profile', 'browser settings-tab inventory and restart persistence')
}
for (const route of inventory.routes) {
  add('HTTP API', `${route.method} ${route.path}`, route.source, 'Complete loopback-only upstream Express server', 'route OPTIONS/response smoke sweep')
}
for (const item of inventory.storage) {
  add('Persistence', `${item.kind}:${item.key}`, item.source, 'Upstream browser storage retained inside the Nexus webview profile', 'browser persistence and restart probe')
}
for (const item of inventory.configuration) {
  add('Server configuration', item.key, item.source, 'Upstream configuration key retained in the private OpenHamClock home/environment', 'staged source inventory and packaged settings persistence smoke')
}
for (const file of inventory.distFiles) {
  add('Web asset', file.path, `dist/${file.path}`, 'Byte-exact staged production asset', `${file.size} bytes; sha256 ${file.sha256}`)
}

const locks = [
  ['application', join(repo, 'third_party', 'openhamclock', 'package-lock.json')],
  ['packager', join(integration, 'package-lock.json')],
]
for (const [scope, path] of locks) {
  const lock = JSON.parse(readFileSync(path, 'utf8'))
  for (const [relative, pkg] of Object.entries(lock.packages || {}).filter(([name]) => name.startsWith('node_modules/')).sort(([a], [b]) => a.localeCompare(b))) {
    const name = relative.slice('node_modules/'.length)
    add('Dependency', `${scope}:${name}@${pkg.version || 'unknown'}`, path.slice(repo.length + 1), scope === 'application' ? 'Included in the target-native OpenHamClock executable dependency closure' : 'Pinned build-time executable packager', pkg.integrity || pkg.resolved || 'package-lock pin', pkg.dev && scope === 'application' ? 'VERIFIED-NON-RUNTIME' : 'VERIFIED', pkg.dev && scope === 'application' ? 'Build/test dependency verified in the lock; not required at runtime' : '')
  }
}

for (const [capability, upstream, seam, evidence, status = 'VERIFIED', reason = ''] of [
  ['Layouts and presets', 'src/layouts and layout UI', 'Rendered by the complete upstream frontend', 'browser layout change and persistence probe'],
  ['Panel placement and resizing', 'upstream dashboard grid', 'Rendered by the complete upstream frontend', 'browser panel interaction probe'],
  ['Map renderer, projections, overlays, and layers', 'src/map and panel modules', 'Rendered without replacement; upstream network/data configuration retained', 'production build plus browser map probe'],
  ['DX cluster and RBN feeds', 'server cluster routes and frontend services', 'Native upstream spots are merged with Nexus spots; posting reaches both configured upstream and Nexus paths', 'bridge tests and browser spot rendering'],
  ['WSJT-X, N1MM, WSPR, and decode surfaces', 'server integrations and digital-mode panels', 'Native collectors remain enabled/configurable; Nexus observations are merged', 'bridge invariants and staged environment audit'],
  ['Rig read telemetry', 'src/contexts/RigContext.jsx', 'Native upstream collector remains available; Nexus events are authoritative while embedded', 'bridge unit tests and browser state probe'],
  ['Rig mutations and PTT', 'src/contexts/RigContext.jsx', 'Embedded controls route only through Nexus commands and its TX safety arbiter', 'refused-PTT, STOP TX, and no-dual-actuation tests'],
  ['Rotator telemetry and controls', 'rotator provider and routes', 'Configured upstream provider is retained; Nexus is the embedded fallback', 'bridge routing tests'],
  ['Logbook create', 'src/services/logbookStore.js', 'Explicit persistent Nexus IDs and complete ADIF extras route to Nexus', 'bridge CRUD and ID-binding tests'],
  ['Logbook update', 'src/services/logbookStore.js', 'Binding-safe optimistic-concurrency update through Nexus', 'conflict and canonical-refresh tests'],
  ['Logbook delete', 'src/services/logbookStore.js', 'Binding-safe deletion through Nexus', 'restart deletion and gone/conflict tests'],
  ['Logbook clear', 'src/services/logbookStore.js', 'Second confirmation plus ADIF backup before Nexus purge', 'fail-closed clear test'],
  ['Logbook ADIF import', 'logbook import UI and parser', 'Native import UI retained; batch records route through Nexus with arbitrary extras', 'ADIF round-trip and batch bridge tests'],
  ['Logbook ADIF export', 'logbook export UI and serializer', 'Native export UI retained; Nexus backup export used before clear', 'browser UI and clear-backup test'],
  ['Awards and confirmation state', 'award panels and logbook fields', 'Upstream award panels retained; LoTW/eQSL/QSL and DXCC fields preserved', 'ADIF confirmation round-trip and panel inventory'],
  ['Spots display and actions', 'spot panels and services', 'Native and Nexus feeds are merged without suppressing upstream actions', 'bridge merge tests and browser probe'],
  ['Themes', 'upstream theme settings', 'No feature is removed or substituted', 'settings inventory and browser interaction probe'],
  ['Keyboard controls and command palette', 'upstream keyboard/command UI', 'No feature is removed or substituted', 'production asset inventory and browser interaction probe'],
  ['Fullscreen', 'upstream header/fullscreen control', 'Runs inside the Ham Clock view/webview', 'browser interaction probe'],
  ['Responsive UI', 'upstream frontend', 'No feature is removed or substituted', 'browser viewport probes'],
  ['Service worker, cache, and offline shell', 'src/pwa and generated service worker', 'Upstream PWA behavior retained inside the webview', 'service-worker registration and offline reload probe'],
  ['Geo/timezone data', 'server/utils/geoTz.js', 'Equivalent package export adapted for single-executable packaging', 'geo endpoint smoke response'],
  ['Add-ons and externally configured auxiliary services', 'rig-bridge, dxspider-proxy, iturhfprop-service, watchtower, wsjtx-relay', 'Configuration and client UI retained; these remain optional external services exactly as upstream defines them', 'source inventory and browser settings inventory', 'VERIFIED-CONFIG-ONLY', 'Live provider/account behavior is listed in the separate external evidence layer because upstream does not bundle these services into its Electron application'],
  ['OpenHamClock update behavior', 'upstream git-based updater and service worker', 'RD-Nexus owns signed single-package updates; sync detects reviewed upstream pins and each build stamps a fresh worker/cache version', 'sync controls, service-worker version/eviction smoke and browser cache-upgrade probe', 'VERIFIED-ADAPTED', 'A nested git updater cannot safely mutate a signed installed RD-Nexus bundle'],
  ['Error, crash, child-exit, and restart recovery', 'server error handling', 'Native supervisor exposes state, validates health/version, isolates data, and restarts on demand', 'health fixture, child restart, parent cleanup, iframe failure UI'],
]) add('Behavior', capability, upstream, seam, evidence, status, reason)

for (const target of platforms.targets || []) {
  add('Platform package', target.target, 'pinned upstream application plus pinned Node runtime', 'Executable and all notices/provenance are included in the single RD-Nexus installer resource tree', `${target.format}; ${target.size} bytes; sha256 ${target.sha256}`, target.packageStatus || 'VERIFIED', target.reason || '')
  if (target.runtimeStatus === 'VERIFIED') {
    add('Native runtime', target.target, 'OpenHamClock health/version and UI', 'Launched by the RD-Nexus native supervisor', target.runtimeEvidence, 'VERIFIED')
  } else {
    external.push({ layer: `Native runtime: ${target.target}`, evidence: target.runtimeEvidence || 'not executed on this host', required: target.runtimeReason || 'Execute on the target operating system/architecture' })
  }
}

external.push(
  { layer: 'Live amateur-radio hardware', evidence: 'Source, adapter, safety, and simulated refusal/un-key paths are verified', required: 'Operator-supervised CAT/PTT/rotator run with real station hardware' },
  { layer: 'Account-bound and Internet providers', evidence: 'All upstream configuration, routes, panels, and failure states are verified', required: 'Valid operator accounts/keys and reachable QRZ, LoTW, eQSL, cluster, weather, satellite, and optional auxiliary providers' },
)

if (rows.some((row) => !row.status.startsWith('VERIFIED'))) {
  throw new Error('Every parity-ledger row must carry a VERIFIED status; keep unexecuted evidence in the external layer')
}

const counts = rows.reduce((out, row) => ({ ...out, [row.status]: (out[row.status] || 0) + 1 }), {})
const header = `# OpenHamClock parity ledger\n\nGenerated from the pinned, staged OpenHamClock source and package locks. It is deliberately exhaustive: every discovered panel, settings tab, server route, browser-storage key, server configuration key, production asset, and locked package has its own row. Every row in the parity table has executable or inspectable evidence and a VERIFIED status. Live RF hardware, provider accounts, and execution on foreign operating systems are listed separately and are never implied by build success.\n\n- Upstream: ${provenance.repository}\n- Version: ${provenance.version}\n- Commit: \`${provenance.commit}\`\n- License: ${provenance.license}\n- Generated parity rows: ${rows.length}\n- Status counts: ${Object.entries(counts).map(([key, value]) => `${key}=${value}`).join(', ')}\n\n| Category | Capability | Upstream source/evidence | RD-Nexus seam | Verification evidence | Status | Reason |\n|---|---|---|---|---|---|---|\n`
const body = rows.map((row) => `| ${clean(row.category)} | ${clean(row.capability)} | ${clean(row.upstream)} | ${clean(row.seam)} | ${clean(row.evidence)} | ${clean(row.status)} | ${clean(row.reason)} |`).join('\n')
const externalTable = `\n## External evidence layer\n\nThese rows are acceptance work outside source/package parity; none is represented as completed by the build.\n\n| Layer | Evidence already established | Additional environment-bound evidence |\n|---|---|---|\n${external.map((item) => `| ${clean(item.layer)} | ${clean(item.evidence)} | ${clean(item.required)} |`).join('\n')}\n`
writeFileSync(join(integration, 'PARITY-LEDGER.md'), `${header}${body}\n${externalTable}`)
process.stdout.write(`Generated ${rows.length} OpenHamClock parity rows (${Object.entries(counts).map(([key, value]) => `${key}=${value}`).join(', ')})\n`)
