#!/usr/bin/env node
// Fetch and fast-forward the OpenHamClock submodule, then report the exact candidate pin.
// Updating integrations/openhamclock/upstream.json remains a deliberate reviewed edit.

import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = join(repo, 'third_party', 'openhamclock')
const run = (args, opts = {}) => execFileSync('git', ['-C', source, ...args], { stdio: 'inherit', ...opts })

run(['fetch', 'origin', 'main', '--tags'])
run(['checkout', '--detach', 'origin/main'])
const commit = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
const version = execFileSync('node', ['-p', "require('./package.json').version"], { cwd: source, encoding: 'utf8' }).trim()
process.stdout.write(`\nOpenHamClock update candidate:\n  commit: ${commit}\n  version: ${version}\n`)
process.stdout.write('Review upstream changes and licenses, then update integrations/openhamclock/upstream.json.\n')
