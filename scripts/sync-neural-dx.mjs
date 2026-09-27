#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = join(repo, 'third_party', 'neural-dx-watcher')
const run = (args) => execFileSync('git', ['-C', source, ...args], { stdio: 'inherit' })
run(['fetch', 'origin', 'main', '--tags'])
run(['checkout', '--detach', 'origin/main'])
const commit = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
process.stdout.write(`\nNeural DX update candidate: ${commit}\n`)
process.stdout.write('Run the route/parity audit, review source and permission scope, then update docs/neural-dx/upstream.json deliberately.\n')
