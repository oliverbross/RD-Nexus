#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const release = readFileSync(new URL('../.github/workflows/release.yml', import.meta.url), 'utf8')
const calls = release.match(/node scripts\/check-openhamclock-package\.mjs/g) || []
assert.equal(calls.length, 4, 'release must verify .deb, AppImage, NSIS, and macOS app package bytes')
for (const invariant of [
  '--target linux-x64 --bundle-resources /tmp/dc/usr/lib/Nexus',
  '--target linux-x64 --bundle-resources squashfs-root/usr/lib/Nexus',
  '--target win-x64 --bundle-resources "$nsis_root"',
  '--target macos-arm64 --bundle-resources "$app/Contents/Resources"',
]) {
  assert.ok(release.replaceAll('\\\n', ' ').includes(invariant), `release package gate missing: ${invariant}`)
}
assert.match(release, /7z x -y -o\/tmp\/nexus-nsis-check/)
assert.match(release, /resources\/openhamclock\/build-receipt\.json/)
process.stdout.write('OpenHamClock same-run package gates cover .deb, AppImage, NSIS, and macOS app artifacts.\n')
