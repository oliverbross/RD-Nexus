#!/usr/bin/env node

import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createBuildReceipt } from './openhamclock-build-receipt.mjs'
import { verifyPackaged } from './check-openhamclock-package.mjs'

const root = mkdtempSync(join(tmpdir(), 'openhamclock-package-check-'))
try {
  const target = 'macos-arm64'
  const expected = { repository: 'https://github.com/accius/openhamclock.git', commit: 'pin', version: '26.8.0' }
  const staged = join(root, 'src-tauri', 'resources', 'openhamclock')
  const bundleResources = join(root, 'bundle', 'Contents', 'Resources')
  const packaged = join(bundleResources, 'resources', 'openhamclock')
  const integration = join(root, 'integrations', 'openhamclock')
  mkdirSync(staged, { recursive: true })
  mkdirSync(packaged, { recursive: true })
  mkdirSync(integration, { recursive: true })
  writeFileSync(join(integration, 'upstream.json'), JSON.stringify(expected))
  const binary = Buffer.alloc(10_000_001)
  binary.writeUInt32LE(0xfeedfacf, 0)
  binary.writeUInt32LE(0x0100000c, 4)
  writeFileSync(join(staged, 'openhamclock'), binary)
  writeFileSync(join(packaged, 'openhamclock'), binary)
  const receipt = createBuildReceipt({ executable: join(staged, 'openhamclock'), target, manifest: expected, generatedAt: '2026-09-28T00:00:00.000Z' })
  writeFileSync(join(staged, 'build-receipt.json'), JSON.stringify(receipt))
  writeFileSync(join(packaged, 'build-receipt.json'), JSON.stringify(receipt))
  assert.equal(verifyPackaged({ repo: root, bundleResources, target }).receiptId, receipt.receiptId)

  const stale = { ...receipt, generatedAt: '2026-09-27T00:00:00.000Z' }
  writeFileSync(join(packaged, 'build-receipt.json'), JSON.stringify(stale))
  assert.throws(() => verifyPackaged({ repo: root, bundleResources, target }), /receipt differs/)
  writeFileSync(join(packaged, 'build-receipt.json'), readFileSync(join(staged, 'build-receipt.json')))
  const changed = Buffer.from(binary)
  changed[4096] = 1
  writeFileSync(join(packaged, 'openhamclock'), changed)
  assert.throws(() => verifyPackaged({ repo: root, bundleResources, target }), /same-run build receipt/)
  process.stdout.write('OpenHamClock packaged artifacts reject stale receipts and mismatched executable bytes.\n')
} finally {
  rmSync(root, { recursive: true, force: true })
}
