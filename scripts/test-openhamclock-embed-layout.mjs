#!/usr/bin/env node
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const read = (path) => readFileSync(join(root, path), 'utf8')
const stage = read('scripts/stage-openhamclock.mjs')
const bridge = read('integrations/openhamclock/nexusBridge.js')
const css = read('integrations/openhamclock/nexusEmbed.css')

for (const adaptation of [
  'nexusEmbed.css',
  'nexus-modern-layout',
  'nexus-modern-sidebar',
  'nexus-map-viewport',
  'nexus-solar-panel',
  'nexus-world-map',
  'nexus-projection-switch',
]) {
  assert.ok(stage.includes(adaptation), `staging adaptation missing: ${adaptation}`)
  if (adaptation !== 'nexusEmbed.css') assert.ok(css.includes(adaptation), `embedded stylesheet missing: ${adaptation}`)
}

assert.match(bridge, /documentElement\.dataset\.nexusEmbed = 'true'/)
assert.match(bridge, /--nexus-embed-vh/)
assert.match(bridge, /window\.addEventListener\('resize'/)
assert.match(css, /html\[data-nexus-embed='true'\]/)
assert.match(css, /grid-template-rows:\s*auto minmax\(0, 1fr\)/)
assert.match(css, /\.nexus-solar-panel img\[alt='SDO Solar Image'\]/)

const anchors = [
  ['third_party/openhamclock/src/layouts/ModernLayout.jsx', "  const mapComponent = (style) => (\n    <div style={{ position: 'relative', borderRadius: '6px', overflow: 'hidden', ...style }}>", 1],
  ['third_party/openhamclock/src/layouts/ModernLayout.jsx', "        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', overflowY: 'auto', overflowX: 'hidden' }}>", 1],
  ['third_party/openhamclock/src/layouts/ModernLayout.jsx', "        <div style={{ display: 'flex', flexDirection: 'column', gap: '4px', overflow: 'hidden' }}>", 1],
  ['third_party/openhamclock/src/components/SolarPanel.jsx', '      className="panel"\n      style={{', 1],
  ['third_party/openhamclock/src/components/WorldMap.jsx', "    <div style={{ position: 'relative', height: '100%', minHeight: '200px' }}>", 1],
  ['third_party/openhamclock/src/components/WorldMap.jsx', "                key={key}\n                onClick={() => {", 1],
]

for (const [path, anchor, expected] of anchors) {
  const count = read(path).split(anchor).length - 1
  assert.equal(count, expected, `${path} integration anchor count changed`)
}

process.stdout.write('OpenHamClock embedded viewport, solar and projection adaptations are anchored.\n')
