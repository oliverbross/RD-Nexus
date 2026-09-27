#!/usr/bin/env node

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createBuildReceipt, executableNameFor } from './openhamclock-build-receipt.mjs'
import { expectedTarget } from './check-openhamclock-staged.mjs'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const target = expectedTarget()
const stage = join(repo, 'src-tauri', 'resources', 'openhamclock')
const manifest = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), 'utf8'))
const executable = join(stage, executableNameFor(target))
const receipt = createBuildReceipt({ executable, target, manifest })
const provenancePath = join(stage, 'provenance.json')
const provenance = JSON.parse(readFileSync(provenancePath, 'utf8'))
writeFileSync(provenancePath, JSON.stringify({ ...provenance, executableSha256: receipt.executable.sha256 }, null, 2) + '\n')
writeFileSync(join(stage, 'build-receipt.json'), JSON.stringify(receipt, null, 2) + '\n')
process.stdout.write(`Recorded OpenHamClock ${target} build receipt ${receipt.receiptId}\n`)
