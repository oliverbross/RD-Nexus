import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const read = (path) => readFileSync(join(root, path), 'utf8')

function workflowJob(workflow, name) {
  const start = workflow.indexOf(`\n  ${name}:\n`)
  assert.notEqual(start, -1, `missing workflow job ${name}`)
  const rest = workflow.slice(start + 1)
  const next = rest.slice(rest.indexOf('\n') + 1).search(/^  [a-zA-Z0-9_-]+:\n/m)
  return next < 0 ? rest : rest.slice(0, rest.indexOf('\n') + 1 + next)
}

test('macOS CI packages and verifies the non-distributed Neural DX companion', () => {
  const ci = read('.github/workflows/ci.yml')
  const job = workflowJob(ci, 'macos-check')
  for (const required of [
    'submodules: recursive',
    "python-version: '3.13'",
    'NEURAL_DX_PRIVATE_BUILD: "1"',
    'node scripts/stage-neural-dx.mjs',
    'node scripts/check-neural-dx-staged.mjs',
    'node scripts/check-neural-dx-consumer.mjs',
    'bash scripts/build-macos.sh --debug',
  ]) {
    assert.ok(job.includes(required), `macos-check lost: ${required}`)
  }
  assert.ok(!job.includes('actions/upload-artifact'), 'the private package rehearsal must never upload its app')
})

test('the macOS build script checks the companion inside the assembled app', () => {
  const script = read('scripts/build-macos.sh')
  for (const required of [
    'Contents/Resources/resources/neural-dx',
    'neural-dx-watcher',
    'provenance.json',
    'upstream.json',
    'NEURAL_DX_SELF_TEST=1',
    "file \"$neural\" | grep -q 'Mach-O.*arm64'",
  ]) {
    assert.ok(script.includes(required), `build-macos.sh lost: ${required}`)
  }
})

test('public release cannot inherit the private-build escape hatch', () => {
  const release = read('.github/workflows/release.yml')
  assert.ok(!release.includes('NEURAL_DX_PRIVATE_BUILD'), 'release.yml bypasses the redistribution gate')
  assert.ok(release.includes('node scripts/check-neural-dx-redistribution.mjs'))
  assert.ok(release.includes('needs: [release-prerequisites, neural-dx-license]'))
})
