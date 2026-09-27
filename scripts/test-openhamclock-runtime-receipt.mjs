#!/usr/bin/env node

import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { REQUIRED_BROWSER_CHECKS, validateRuntimeReceipt } from './openhamclock-runtime-receipt.mjs'
import { createBuildReceipt } from './openhamclock-build-receipt.mjs'

const repo = mkdtempSync(join(tmpdir(), 'openhamclock-runtime-receipt-'))
try {
  const resource = join(repo, 'src-tauri', 'resources', 'openhamclock')
  mkdirSync(resource, { recursive: true })
  const executable = join(resource, 'openhamclock')
  const executableBytes = Buffer.alloc(10_000_001)
  executableBytes.writeUInt32LE(0xfeedfacf, 0)
  executableBytes.writeUInt32LE(0x0100000c, 4)
  writeFileSync(executable, executableBytes)
  const executableSha256 = createHash('sha256').update(executableBytes).digest('hex')
  const expected = { repository: 'https://github.com/accius/openhamclock.git', commit: 'pin', version: '26.8.0' }
  mkdirSync(join(repo, 'integrations', 'openhamclock'), { recursive: true })
  writeFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), JSON.stringify(expected))
  writeFileSync(join(resource, 'provenance.json'), JSON.stringify({ ...expected, target: 'macos-arm64' }))
  const buildReceipt = createBuildReceipt({ executable, target: 'macos-arm64', manifest: expected, generatedAt: '2026-09-28T00:00:00.000Z' })
  writeFileSync(join(resource, 'build-receipt.json'), JSON.stringify(buildReceipt))
  const receipt = {
    schemaVersion: 2,
    receiptId: 'fixture-receipt',
    candidateSha: 'candidate-a',
    target: 'macos-arm64',
    buildReceiptId: buildReceipt.receiptId,
    executableSha256,
    provenance: { commit: 'pin', version: '26.8.0', target: 'macos-arm64' },
    smoke: { passed: true, exitCode: 0, outputSha256: 'a'.repeat(64) },
    browser: {
      passed: true,
      evidenceSha256: 'b'.repeat(64),
      checks: Object.fromEntries(REQUIRED_BROWSER_CHECKS.map((name) => [name, true])),
    },
  }
  const targetEntry = { target: 'macos-arm64', sourceCommit: 'pin' }
  assert.equal(validateRuntimeReceipt({ repo, receipt, candidateSha: 'candidate-a', targetEntry }).valid, true)
  assert.match(validateRuntimeReceipt({ repo, receipt, candidateSha: 'candidate-b', targetEntry }).reason, /SHA is stale/)
  assert.match(validateRuntimeReceipt({ repo, receipt: { ...receipt, executableSha256: 'c'.repeat(64) }, candidateSha: 'candidate-a', targetEntry }).reason, /hash .*stale/)
  assert.match(validateRuntimeReceipt({ repo, receipt: { ...receipt, buildReceiptId: 'stale' }, candidateSha: 'candidate-a', targetEntry }).reason, /build receipt is stale/)
  assert.match(validateRuntimeReceipt({ repo, receipt: { ...receipt, browser: { ...receipt.browser, checks: { ...receipt.browser.checks, offlineReloadRendered: false } } }, candidateSha: 'candidate-a', targetEntry }).reason, /incomplete browser/)
  writeFileSync(executable, 'copied different executable')
  assert.match(validateRuntimeReceipt({ repo, receipt, candidateSha: 'candidate-a', targetEntry }).reason, /build evidence is invalid/)
  process.stdout.write('OpenHamClock runtime receipts reject stale SHA, copied executables, and incomplete browser evidence.\n')
} finally {
  rmSync(repo, { recursive: true, force: true })
}
