#!/usr/bin/env node
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const read = (path) => readFileSync(join(root, path), 'utf8')
const upstream = JSON.parse(read('integrations/openhamclock/upstream.json'))
const actual = execFileSync('git', ['-C', join(root, 'third_party/openhamclock'), 'rev-parse', 'HEAD'], {
  encoding: 'utf8',
}).trim()
assert.equal(actual, upstream.commit, 'OpenHamClock submodule must match the reviewed revision')
assert.equal(JSON.parse(read('third_party/openhamclock/package.json')).version, upstream.version)
assert.match(read('third_party/openhamclock/LICENSE'), /MIT License/)
const pkgPatches = JSON.parse(read('integrations/openhamclock/node_modules/@yao-pkg/pkg-fetch/patches/patches.json'))
assert.ok(pkgPatches[`v${upstream.nodeRuntime.version}`], 'pinned pkg tool must embed the reviewed Node.js runtime')

const stage = read('scripts/stage-openhamclock.mjs')
for (const invariant of [
  'app.listen(PORT, HOST', 'frameguard', 'geo-tz/dist/find-all',
  'process.env[key] === undefined', 'nexusBridge.js', 'replaceFromNexus',
  'nexus-rig-state', 'OHC_NEXUS_PARENT_PID', '@yao-pkg', 'process.execPath',
]) assert.ok(stage.includes(invariant), 'staging adaptation missing: ' + invariant)

const supervisor = read('src-tauri/src/openhamclock.rs')
for (const invariant of ['127.0.0.1', 'phase: "idle"', 'CREATE_NO_WINDOW', 'WSJTX_ENABLED', 'N1MM_UDP_ENABLED']) {
  assert.ok(supervisor.includes(invariant), 'supervisor invariant missing: ' + invariant)
}

const tauri = JSON.parse(read('src-tauri/tauri.conf.json'))
assert.equal(tauri.bundle.resources['resources/openhamclock/*'], 'resources/openhamclock/')
assert.match(tauri.build.beforeBuildCommand, /check-openhamclock-staged/)

const nav = read('ui/src/components/ModeNav.tsx')
assert.ok(nav.indexOf('navBtn(HAM_CLOCK)') < nav.indexOf('navBtn(PHONE)'), 'Ham Clock must appear above Phone')
assert.match(read('ui/src/App.tsx'), /<HamClockView snap={snap} onSnap={setSnap}/)
const bridge = read('integrations/openhamclock/nexusBridge.js')
for (const channel of ['logbook', 'dxcluster', 'rotator', 'wsjtx', 'nexus-rig-state']) {
  assert.ok(bridge.includes(channel), 'Nexus bridge channel missing: ' + channel)
}
assert.ok(statSync(join(root, 'third_party/openhamclock')).isDirectory())
process.stdout.write('OpenHamClock integration pin, bundle, navigation, and bridge invariants pass (' + actual + ').\n')
