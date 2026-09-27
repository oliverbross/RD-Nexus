#!/usr/bin/env node

import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, truncateSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { expectedTarget, verifyStaged } from './check-openhamclock-staged.mjs'
import { createBuildReceipt, receiptId } from './openhamclock-build-receipt.mjs'

const upstream = JSON.parse(readFileSync(new URL('../integrations/openhamclock/upstream.json', import.meta.url), 'utf8'))

function binaryHeader(target) {
  const header = Buffer.alloc(512)
  if (target === 'win-x64') {
    header.write('MZ', 0, 'ascii')
    header.writeUInt32LE(0x80, 0x3c)
    header.write('PE\0\0', 0x80, 'binary')
    header.writeUInt16LE(0x8664, 0x84)
  } else if (target.startsWith('linux-')) {
    Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1]).copy(header)
    header.writeUInt16LE(target === 'linux-arm64' ? 0xb7 : 0x3e, 18)
  } else {
    header.writeUInt32LE(0xfeedfacf, 0)
    header.writeUInt32LE(target === 'macos-arm64' ? 0x0100000c : 0x01000007, 4)
  }
  return header
}

function fixture(target, binaryTarget = target, binaryName = target === 'win-x64' ? 'openhamclock.exe' : 'openhamclock') {
  const fixtureUpstream = structuredClone(upstream)
  const repo = mkdtempSync(join(tmpdir(), 'openhamclock-stage-check-'))
  const integration = join(repo, 'integrations', 'openhamclock')
  const stage = join(repo, 'src-tauri', 'resources', 'openhamclock')
  mkdirSync(integration, { recursive: true })
  mkdirSync(stage, { recursive: true })
  writeFileSync(join(integration, 'upstream.json'), JSON.stringify(fixtureUpstream))
  const executable = join(stage, binaryName)
  writeFileSync(executable, binaryHeader(binaryTarget))
  truncateSync(executable, 10_000_001)
  const executableSha256 = createHash('sha256').update(readFileSync(executable)).digest('hex')
  writeFileSync(join(stage, 'OPENHAMCLOCK-LICENSE.txt'), Buffer.alloc(501))
  writeFileSync(join(stage, 'OPENHAMCLOCK-THIRD-PARTY-NOTICES.txt'), Buffer.alloc(10_001))
  writeFileSync(join(stage, 'NODE-RUNTIME-LICENSE.txt'), Buffer.alloc(50_001))
  fixtureUpstream.nodeRuntime.licenseSha256 = createHash('sha256').update(readFileSync(join(stage, 'NODE-RUNTIME-LICENSE.txt'))).digest('hex')
  writeFileSync(join(integration, 'upstream.json'), JSON.stringify(fixtureUpstream))
  writeFileSync(join(stage, 'provenance.json'), JSON.stringify({ ...fixtureUpstream, target, executable: binaryName, executableSha256 }))
  const builtReceipt = createBuildReceipt({
    executable,
    target: binaryTarget,
    manifest: fixtureUpstream,
    generatedAt: '2026-09-28T00:00:00.000Z',
  })
  const receipt = { ...builtReceipt, target }
  receipt.receiptId = receiptId(receipt)
  writeFileSync(join(stage, 'build-receipt.json'), JSON.stringify(receipt))
  const rows = (length) => Array.from({ length }, () => ({}))
  writeFileSync(join(stage, 'parity-inventory.json'), JSON.stringify({
    panels: rows(50), settingsTabs: rows(10), routes: rows(80), storage: rows(50), configuration: rows(80), distFiles: rows(20),
  }))
  return repo
}

const roots = []
try {
  for (const target of ['win-x64', 'linux-x64', 'linux-arm64', 'macos-x64', 'macos-arm64']) {
    const good = fixture(target)
    roots.push(good)
    assert.equal(verifyStaged({ repo: good, target }).target, target)
  }

  const wrongName = fixture('win-x64')
  roots.push(wrongName)
  assert.throws(() => verifyStaged({ repo: wrongName, target: 'macos-arm64' }), /macos-arm64 is not staged/)

  const renamedPe = fixture('macos-arm64', 'win-x64')
  roots.push(renamedPe)
  assert.throws(() => verifyStaged({ repo: renamedPe, target: 'macos-arm64' }), /not a macOS Mach-O binary/)

  const staleSibling = fixture('macos-arm64')
  roots.push(staleSibling)
  writeFileSync(join(staleSibling, 'src-tauri', 'resources', 'openhamclock', 'openhamclock.exe'), binaryHeader('win-x64'))
  assert.throws(() => verifyStaged({ repo: staleSibling, target: 'macos-arm64' }), /Wrong-platform/)

  const staleReceipt = fixture('macos-arm64')
  roots.push(staleReceipt)
  const staleReceiptPath = join(staleReceipt, 'src-tauri', 'resources', 'openhamclock', 'build-receipt.json')
  const stale = JSON.parse(readFileSync(staleReceiptPath, 'utf8'))
  writeFileSync(staleReceiptPath, JSON.stringify({ ...stale, receiptId: 'stale' }))
  assert.throws(() => verifyStaged({ repo: staleReceipt, target: 'macos-arm64' }), /receipt digest mismatch/)

  const changedAfterReceipt = fixture('macos-arm64')
  roots.push(changedAfterReceipt)
  const changedExecutable = join(changedAfterReceipt, 'src-tauri', 'resources', 'openhamclock', 'openhamclock')
  const changedBytes = readFileSync(changedExecutable)
  changedBytes[4096] = 1
  writeFileSync(changedExecutable, changedBytes)
  assert.throws(() => verifyStaged({ repo: changedAfterReceipt, target: 'macos-arm64' }), /checksum mismatch|same-run build receipt/)

  assert.equal(expectedTarget([process.execPath, 'check-openhamclock-staged.mjs', '--target', 'win-x64'], {}, 'darwin', 'arm64'), 'win-x64')
  assert.equal(expectedTarget([], { TAURI_ENV_TARGET_TRIPLE: 'aarch64-apple-darwin' }, 'linux', 'x64'), 'macos-arm64')
  process.stdout.write('OpenHamClock staged-resource checker rejects wrong target names, formats, provenance, and siblings.\n')
} finally {
  for (const root of roots) rmSync(root, { recursive: true, force: true })
}
