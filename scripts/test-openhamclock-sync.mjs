#!/usr/bin/env node
import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const temp = mkdtempSync(join(tmpdir(), 'ohc-sync-control-'))
const source = join(temp, 'source')
const manifest = join(temp, 'manifest.json')
const git = (args) => execFileSync('git', ['-C', source, ...args], { encoding: 'utf8' }).trim()
try {
  execFileSync('git', ['init', '-q', source])
  git(['config', 'user.name', 'Parity Test'])
  git(['config', 'user.email', 'parity@example.invalid'])
  writeFileSync(join(source, 'package.json'), '{"version":"1.0.0"}\n')
  git(['add', 'package.json'])
  git(['commit', '-qm', 'one'])
  const reviewed = git(['rev-parse', 'HEAD'])
  writeFileSync(manifest, JSON.stringify({ commit: reviewed }))
  const positive = spawnSync(process.execPath, [join(root, 'scripts/sync-openhamclock.mjs'), '--source', source, '--manifest', manifest, '--ref', 'HEAD', '--no-fetch', '--require-current', '--json'], { encoding: 'utf8' })
  assert.equal(positive.status, 0, positive.stderr)
  assert.equal(JSON.parse(positive.stdout).status, 'current')

  writeFileSync(join(source, 'package.json'), '{"version":"1.1.0"}\n')
  git(['add', 'package.json'])
  git(['commit', '-qm', 'two'])
  const before = git(['rev-parse', 'HEAD'])
  const negative = spawnSync(process.execPath, [join(root, 'scripts/sync-openhamclock.mjs'), '--source', source, '--manifest', manifest, '--ref', 'HEAD', '--no-fetch', '--require-current', '--json'], { encoding: 'utf8' })
  assert.equal(negative.status, 2, negative.stderr)
  const report = JSON.parse(negative.stdout)
  assert.equal(report.status, 'update-available')
  assert.equal(report.reviewedCommit, reviewed)
  assert.equal(report.candidateCommit, before)
  assert.equal(git(['rev-parse', 'HEAD']), before, 'negative control must not mutate the checkout')
  process.stdout.write('OpenHamClock sync positive and negative controls pass.\n')
} finally {
  rmSync(temp, { recursive: true, force: true })
}
