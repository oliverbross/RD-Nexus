#!/usr/bin/env node
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const locks = [
  join(root, 'third_party', 'openhamclock', 'package-lock.json'),
  join(root, 'integrations', 'openhamclock', 'package-lock.json'),
]
const allowed = new Set([
  '0BSD', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'BlueOak-1.0.0',
  'CC-BY-4.0', 'CC0-1.0', 'GPL-3.0-only', 'ISC', 'MIT', 'MIT-0',
  'MPL-2.0', 'Python-2.0', 'Unlicense', 'WTFPL',
])
const licenses = new Map()
const problems = []

for (const path of locks) {
  const lock = JSON.parse(readFileSync(path, 'utf8'))
  for (const [name, pkg] of Object.entries(lock.packages ?? {})) {
    const expression = typeof pkg.license === 'string' ? pkg.license.trim() : ''
    const label = name || lock.name || path
    if (!expression) {
      problems.push(label + ': no license declared in package-lock.json')
      continue
    }
    const ids = expression.replace(/[()]/g, ' ').split(/\s+(?:OR|AND|WITH)\s+/)
    if (!ids.some((id) => allowed.has(id.trim()))) {
      problems.push(label + ': unreviewed license expression "' + expression + '"')
    }
    const names = licenses.get(expression) ?? []
    names.push(label)
    licenses.set(expression, names)
  }
}

for (const [license, packages] of [...licenses].sort(([a], [b]) => a.localeCompare(b))) {
  process.stdout.write(license + ': ' + packages.length + '\n')
}
if (problems.length) {
  process.stderr.write('OpenHamClock license review failed:\n' + problems.map((line) => '- ' + line).join('\n') + '\n')
  process.exit(1)
}
process.stdout.write('Reviewed ' + [...licenses.values()].reduce((sum, names) => sum + names.length, 0) + ' package records.\n')
