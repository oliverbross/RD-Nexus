#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const release = readFileSync(new URL('../.github/workflows/release.yml', import.meta.url), 'utf8')
const piDockerfile = readFileSync(new URL('./Dockerfile.pi', import.meta.url), 'utf8')
const calls = release.match(/node scripts\/check-openhamclock-package\.mjs/g) || []
assert.equal(calls.length, 4, 'host release jobs must verify desktop .deb, AppImage, NSIS, and macOS app package bytes')
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
assert.match(release, /base: \[bookworm, trixie\]/)
assert.match(piDockerfile, /node scripts\/check-openhamclock-package\.mjs \\\n+\s+--target linux-arm64 --bundle-resources \/tmp\/debcheck\/usr\/lib\/Nexus/)
assert.ok(
  piDockerfile.indexOf('check-openhamclock-package.mjs') > piDockerfile.indexOf('./scripts/build-linux.sh') &&
  piDockerfile.indexOf('check-openhamclock-package.mjs') < piDockerfile.indexOf('cp src-tauri/target/release/bundle/deb/*.deb /out/'),
  'Pi package gate must run after the .deb build and before artifact export',
)
process.stdout.write('OpenHamClock same-run package gates cover desktop .deb, AppImage, NSIS, macOS app, and both Pi .deb matrix artifacts.\n')
