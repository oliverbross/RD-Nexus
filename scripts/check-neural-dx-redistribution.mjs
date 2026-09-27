#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const evidence = join(repo, 'docs', 'neural-dx', 'REDISTRIBUTION_APPROVED.md')
if (!existsSync(evidence)) {
  throw new Error('Neural DX public redistribution is blocked: archive the qualifying written grant in docs/neural-dx/REDISTRIBUTION_APPROVED.md')
}
const text = readFileSync(evidence, 'utf8').toLowerCase()
for (const required of ['modification', 'redistribution', 'sublicensing', 'gpl-3.0-only']) {
  if (!text.includes(required)) throw new Error(`Neural DX redistribution evidence is incomplete: missing ${required}`)
}
process.stdout.write('Neural DX redistribution evidence present\n')
