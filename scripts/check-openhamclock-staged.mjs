#!/usr/bin/env node
// Tauri's resource glob matches the tracked README even when the actual companion is absent.
// Refuse that deceptive green build: the executable, license and exact provenance all ship.

import { createHash } from 'node:crypto'
import { existsSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const dir = join(repo, 'src-tauri', 'resources', 'openhamclock')
const expected = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), 'utf8'))
const candidates = ['openhamclock', 'openhamclock.exe'].map((name) => join(dir, name))
for (const name of ['.env', 'config.json', 'data', 'openhamclock.log', 'cluster-status.json']) {
  if (existsSync(join(dir, name))) throw new Error(`OpenHamClock runtime state must not be bundled: ${name}`)
}
const executable = candidates.find((path) => existsSync(path))
if (!executable || statSync(executable).size < 10_000_000) {
  throw new Error('OpenHamClock executable is not staged; run node scripts/stage-openhamclock.mjs')
}
const license = join(dir, 'OPENHAMCLOCK-LICENSE.txt')
if (!existsSync(license) || statSync(license).size < 500) throw new Error('OpenHamClock MIT license is not staged')
const notices = join(dir, 'OPENHAMCLOCK-THIRD-PARTY-NOTICES.txt')
if (!existsSync(notices) || statSync(notices).size < 10_000) throw new Error('OpenHamClock third-party notices are not staged')
const nodeLicense = join(dir, 'NODE-RUNTIME-LICENSE.txt')
if (!existsSync(nodeLicense) || statSync(nodeLicense).size < 50_000) throw new Error('Node.js runtime license is not staged')
const nodeLicenseSha = createHash('sha256').update(readFileSync(nodeLicense)).digest('hex')
if (nodeLicenseSha !== expected.nodeRuntime.licenseSha256) throw new Error('Node.js runtime license checksum mismatch')
const provenance = JSON.parse(readFileSync(join(dir, 'provenance.json'), 'utf8'))
if (provenance.commit !== expected.commit || provenance.version !== expected.version) {
  throw new Error('OpenHamClock staged provenance does not match the reviewed upstream pin')
}
const inventory = JSON.parse(readFileSync(join(dir, 'parity-inventory.json'), 'utf8'))
if (inventory.panels.length < 50 || inventory.settingsTabs.length < 10 || inventory.routes.length < 80 || inventory.storage.length < 50 || inventory.configuration.length < 80 || inventory.distFiles.length < 20) {
  throw new Error('OpenHamClock staged parity inventory is incomplete')
}
process.stdout.write(`OpenHamClock ${provenance.version} staged (${provenance.commit})\n`)
