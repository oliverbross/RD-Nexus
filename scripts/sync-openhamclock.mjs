#!/usr/bin/env node
// Inspect upstream and optionally move the submodule to an update candidate.
// The reviewed manifest remains the gate: this command never edits it, commits,
// pushes, or publishes. Tests may provide local --source/--manifest/--ref seams.

import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const value = (name, fallback) => {
  const index = args.indexOf(name)
  return index >= 0 && args[index + 1] ? resolve(args[index + 1]) : fallback
}
const source = value('--source', join(repo, 'third_party', 'openhamclock'))
const manifestPath = value('--manifest', join(repo, 'integrations', 'openhamclock', 'upstream.json'))
const candidateRef = (() => {
  const index = args.indexOf('--ref')
  return index >= 0 && args[index + 1] ? args[index + 1] : 'origin/main'
})()
const noFetch = args.includes('--no-fetch')
const apply = args.includes('--apply')
const requireCurrent = args.includes('--require-current')
const json = args.includes('--json')
const git = (gitArgs, options = {}) => execFileSync('git', ['-C', source, ...gitArgs], { encoding: 'utf8', ...options }).trim()

if (!noFetch) execFileSync('git', ['-C', source, 'fetch', 'origin', 'main', '--tags'], { stdio: 'inherit' })
const candidate = git(['rev-parse', candidateRef])
const before = git(['rev-parse', 'HEAD'])
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
const candidateVersion = JSON.parse(git(['show', `${candidate}:package.json`])).version
const status = candidate === manifest.commit ? 'current' : 'update-available'

if (apply && status === 'update-available') {
  execFileSync('git', ['-C', source, 'checkout', '--detach', candidate], { stdio: 'inherit' })
}
const result = {
  status,
  reviewedCommit: manifest.commit,
  candidateCommit: candidate,
  candidateVersion,
  checkoutBefore: before,
  checkoutAfter: git(['rev-parse', 'HEAD']),
  applied: apply && status === 'update-available',
}
if (json) process.stdout.write(JSON.stringify(result) + '\n')
else {
  process.stdout.write(`\nOpenHamClock upstream: ${status}\n`)
  process.stdout.write(`  reviewed:  ${manifest.commit}\n  candidate: ${candidate} (${candidateVersion})\n`)
  process.stdout.write(apply ? 'Candidate checked out; review and update upstream.json deliberately.\n' : 'Use --apply only after reviewing the candidate; upstream.json is never changed automatically.\n')
}
if (requireCurrent && status !== 'current') process.exitCode = 2
