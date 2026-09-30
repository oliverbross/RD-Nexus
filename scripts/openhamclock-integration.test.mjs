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
  'process.env[key] === undefined', 'nexusBridge.js', 'mergeFromNexus',
  'nexusLogMerge.mjs', 'nexusMirrorStore.mjs', 'commitNexusDelta', 'applyNexusDelta', 'NEXUS_LEDGER_KEY', 'nexus-rig-state', 'OHC_NEXUS_PARENT_PID',
  'OHC_NEXUS_PORT_FILE', 'EADDRINUSE', '@yao-pkg', 'process.execPath',
  'configuration', 'nexusEmbed.css', 'nexus-modern-layout', 'nexus-solar-panel',
  'nexus-world-map',
]) assert.ok(stage.includes(invariant), 'staging adaptation missing: ' + invariant)
for (const control of ['rig:setFreq', 'rig:setMode', 'rig:setPTT']) {
  assert.match(stage, new RegExp(control.replace(':', '\\:') + "[^\\n]+return;"), `embedded ${control} must return before upstream actuation`)
}

const supervisor = read('src-tauri/src/openhamclock.rs')
for (const invariant of ['127.0.0.1', 'phase: "idle"', 'use tempo_core::process::command', 'health_ready', 'openhamclock_ptt', 'PortHandshake', 'MAX_HEALTH_RESPONSE_BYTES', 'serde_json::from_slice', 'openhamclock_log_delta', 'log_changed_ids_since']) {
  assert.ok(supervisor.includes(invariant), 'supervisor invariant missing: ' + invariant)
}
const processHelper = read('crates/tempo-core/src/process.rs')
assert.ok(processHelper.includes('cmd.creation_flags(CREATE_NO_WINDOW)'), 'shared process helper must suppress the Windows console window')
assert.ok(!supervisor.includes('.env("WSJTX_ENABLED", "false")'), 'upstream WSJT-X must remain configurable')
assert.ok(!supervisor.includes('.env("N1MM_UDP_ENABLED", "false")'), 'upstream N1MM must remain configurable')

const tauri = JSON.parse(read('src-tauri/tauri.conf.json'))
assert.equal(tauri.bundle.resources['resources/openhamclock/*'], 'resources/openhamclock/')
assert.deepEqual(tauri.build.beforeBuildCommand, {
  cwd: '..',
  script: 'node scripts/check-openhamclock-staged.mjs && npm --prefix ui run build',
})
const packageInspection = JSON.parse(read('integrations/openhamclock/package-inspection.json'))
assert.match(packageInspection.evidenceBoundary, /byte-nondeterministic/)
for (const target of packageInspection.targets) {
  assert.equal(target.contentIdentity, 'BUILD-SPECIFIC')
  assert.equal(Object.hasOwn(target, 'sha256'), false, `${target.target} must not claim a stable executable hash`)
}

const nav = read('ui/src/components/ModeNav.tsx')
assert.ok(nav.indexOf('navBtn(HAM_CLOCK)') < nav.indexOf('navBtn(PHONE)'), 'Ham Clock must appear above Phone')
assert.match(read('ui/src/App.tsx'), /<HamClockView snap={snap} onSnap={setSnap}/)
const bridge = read('integrations/openhamclock/nexusBridge.js')
for (const channel of ['logbook', 'dxcluster', 'rotator', 'wsjtx', 'nexus-rig-state']) {
  assert.ok(bridge.includes(channel), 'Nexus bridge channel missing: ' + channel)
}
assert.match(bridge, /documentElement\.dataset\.nexusEmbed/, 'embedded dashboard must expose its bounded layout mode')
const embedCss = read('integrations/openhamclock/nexusEmbed.css')
for (const selector of ['nexus-modern-layout', 'nexus-solar-panel', 'nexus-world-map']) {
  assert.ok(embedCss.includes(selector), 'embedded layout stylesheet missing: ' + selector)
}
assert.ok(statSync(join(root, 'third_party/openhamclock')).isDirectory())
process.stdout.write('OpenHamClock integration pin, bundle, navigation, and bridge invariants pass (' + actual + ').\n')
