#!/usr/bin/env node
import { readFileSync } from 'node:fs'

const versionAt = process.argv.indexOf('--version')
const expectedVersion = versionAt >= 0 ? process.argv[versionAt + 1] : ''
if (!expectedVersion) throw new Error('usage: check-neural-dx-self-test.mjs --version VERSION')

const line = readFileSync(0, 'utf8')
  .split(/\r?\n/)
  .find((candidate) => candidate.startsWith('NEURAL_DX_SELF_TEST='))
if (!line) throw new Error('bundled Neural DX sidecar did not report its self-test')

const result = JSON.parse(line.slice('NEURAL_DX_SELF_TEST='.length))
if (result.ok !== true || !Array.isArray(result.missing) || result.missing.length !== 0) {
  throw new Error(`bundled Neural DX self-test failed: ${JSON.stringify(result)}`)
}
if (result.version !== expectedVersion) {
  throw new Error(`bundled Neural DX version mismatch: ${result.version} != ${expectedVersion}`)
}

process.stdout.write(`Bundled Neural DX self-test passed (${result.version})\n`)
