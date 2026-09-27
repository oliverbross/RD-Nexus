#!/usr/bin/env node

import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { REQUIRED_BROWSER_CHECKS, validateRuntimeReceipt } from './openhamclock-runtime-receipt.mjs'

const repo = mkdtempSync(join(tmpdir(), 'openhamclock-runtime-receipt-'))
try {
  const resource = join(repo, 'src-tauri', 'resources', 'openhamclock')
  mkdirSync(resource, { recursive: true })
  const executable = join(resource, 'openhamclock')
  writeFileSync(executable, 'macos-arm64 executable fixture')
  const executableSha256 = createHash('sha256').update('macos-arm64 executable fixture').digest('hex')
  writeFileSync(join(resource, 'provenance.json'), JSON.stringify({ commit: 'pin', version: '26.8.0', target: 'macos-arm64' }))
  const receipt = {
    schemaVersion: 1,
    receiptId: 'fixture-receipt',
    candidateSha: 'candidate-a',
    target: 'macos-arm64',
    executableSha256,
    provenance: { commit: 'pin', version: '26.8.0', target: 'macos-arm64' },
    smoke: { passed: true, exitCode: 0, outputSha256: 'a'.repeat(64) },
    browser: {
      passed: true,
      evidenceSha256: 'b'.repeat(64),
      checks: Object.fromEntries(REQUIRED_BROWSER_CHECKS.map((name) => [name, true])),
    },
  }
  const targetEntry = { target: 'macos-arm64', sha256: executableSha256 }
  assert.equal(validateRuntimeReceipt({ repo, receipt, candidateSha: 'candidate-a', targetEntry }).valid, true)
  assert.match(validateRuntimeReceipt({ repo, receipt, candidateSha: 'candidate-b', targetEntry }).reason, /SHA is stale/)
  assert.match(validateRuntimeReceipt({ repo, receipt: { ...receipt, executableSha256: 'c'.repeat(64) }, candidateSha: 'candidate-a', targetEntry }).reason, /hash is stale/)
  assert.match(validateRuntimeReceipt({ repo, receipt: { ...receipt, browser: { ...receipt.browser, checks: { ...receipt.browser.checks, offlineReloadRendered: false } } }, candidateSha: 'candidate-a', targetEntry }).reason, /incomplete browser/)
  writeFileSync(executable, 'copied different executable')
  assert.match(validateRuntimeReceipt({ repo, receipt, candidateSha: 'candidate-a', targetEntry }).reason, /hash is stale/)
  process.stdout.write('OpenHamClock runtime receipts reject stale SHA, copied executables, and incomplete browser evidence.\n')
} finally {
  rmSync(repo, { recursive: true, force: true })
}
