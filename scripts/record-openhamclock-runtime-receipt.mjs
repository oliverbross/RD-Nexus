#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { REQUIRED_BROWSER_CHECKS, sha256, validateRuntimeReceipt } from './openhamclock-runtime-receipt.mjs'
import { validateBuildReceipt } from './openhamclock-build-receipt.mjs'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const value = (name) => {
  const at = process.argv.indexOf(name)
  if (at < 0 || !process.argv[at + 1]) throw new Error(`Missing ${name}`)
  return resolve(process.argv[at + 1])
}
const targetAt = process.argv.indexOf('--target')
const target = targetAt >= 0 ? process.argv[targetAt + 1] : 'macos-arm64'
const smokePath = value('--smoke-log')
const browserPath = value('--browser-report')
const smokeOutput = readFileSync(smokePath, 'utf8')
if (!smokeOutput.includes('OpenHamClock packaged runtime parity smoke passes')) throw new Error('Smoke log has no packaged-runtime pass marker')
const browser = JSON.parse(readFileSync(browserPath, 'utf8'))
if (REQUIRED_BROWSER_CHECKS.some((name) => browser.checks?.[name] !== true)) throw new Error('Browser report is incomplete')
const packageInspection = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'package-inspection.json'), 'utf8'))
const targetEntry = packageInspection.targets.find((entry) => entry.target === target)
if (!targetEntry) throw new Error(`Package inspection has no ${target} entry`)
const executable = join(repo, 'src-tauri', 'resources', 'openhamclock', target === 'win-x64' ? 'openhamclock.exe' : 'openhamclock')
const provenance = JSON.parse(readFileSync(join(repo, 'src-tauri', 'resources', 'openhamclock', 'provenance.json'), 'utf8'))
const buildReceipt = JSON.parse(readFileSync(join(repo, 'src-tauri', 'resources', 'openhamclock', 'build-receipt.json'), 'utf8'))
const expected = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), 'utf8'))
const build = validateBuildReceipt({ receipt: buildReceipt, executable, target, expected })
const candidateSha = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim()
const receipt = {
  schemaVersion: 2,
  receiptId: '',
  generatedAt: new Date().toISOString(),
  candidateSha,
  target,
  buildReceiptId: build.receiptId,
  executableSha256: sha256(executable),
  provenance: { commit: provenance.commit, version: provenance.version, target: provenance.target },
  smoke: { passed: true, exitCode: 0, outputSha256: createHash('sha256').update(smokeOutput).digest('hex') },
  browser: { passed: true, evidenceSha256: createHash('sha256').update(readFileSync(browserPath)).digest('hex'), checks: browser.checks },
}
receipt.receiptId = createHash('sha256').update(JSON.stringify(receipt)).digest('hex').slice(0, 24)
const verdict = validateRuntimeReceipt({ repo, receipt, candidateSha, targetEntry })
if (!verdict.valid) throw new Error(verdict.reason)
const output = join(repo, 'integrations', 'openhamclock', 'runtime-receipt.json')
writeFileSync(output, JSON.stringify(receipt, null, 2) + '\n')
process.stdout.write(`Recorded OpenHamClock runtime receipt ${receipt.receiptId} for ${candidateSha} ${target}\n`)
