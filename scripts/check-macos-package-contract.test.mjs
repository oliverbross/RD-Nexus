import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
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

test('macOS CI stages and verifies both bundled companions', () => {
  const ci = read('.github/workflows/ci.yml')
  const job = workflowJob(ci, 'macos-check')
  for (const required of [
    'submodules: recursive',
    "python-version: '3.13'",
    'NEURAL_DX_PRIVATE_BUILD: "1"',
    'node scripts/stage-neural-dx.mjs',
    'node scripts/check-neural-dx-staged.mjs',
    'node scripts/check-neural-dx-consumer.mjs',
    'node scripts/stage-openhamclock.mjs --target macos-arm64',
    'node scripts/check-openhamclock-staged.mjs --target macos-arm64',
    'bash scripts/build-macos.sh --debug',
  ]) {
    assert.ok(job.includes(required), `macos-check lost: ${required}`)
  }
  assert.ok(!job.includes('actions/upload-artifact'), 'the private package rehearsal must never upload its app')
})

test('the macOS build script checks both companions inside the assembled app', () => {
  const script = read('scripts/build-macos.sh')
  for (const required of [
    'Contents/Resources/resources/neural-dx',
    'neural-dx-watcher',
    'provenance.json',
    'upstream.json',
    'NEURAL_DX_SELF_TEST=1',
    'check-neural-dx-self-test.mjs',
    "file \"$neural\" | grep -q 'Mach-O.*arm64'",
    'Contents/Resources/resources/openhamclock',
    'check-openhamclock-package.mjs',
    '--target macos-arm64 --bundle-resources "$app/Contents/Resources"',
  ]) {
    assert.ok(script.includes(required), `build-macos.sh lost: ${required}`)
  }
})

test('the default release build never expands an empty array under macOS Bash nounset', () => {
  const script = read('scripts/build-macos.sh')
  assert.ok(!script.includes('tauri_args=()'), 'an empty array fails under the system Bash 3.2 with set -u')
  assert.match(script, /tauri_args=\([\s\S]*--features radio,custom-protocol[\s\S]*--bundles app[\s\S]*--config/)
  assert.ok(script.includes('cargo tauri build "${tauri_args[@]}" "$@"'))
})

test('the assembled-app self-test parser accepts Python json.dumps output and rejects bad payloads', () => {
  const checker = join(root, 'scripts', 'check-neural-dx-self-test.mjs')
  const run = (input) => spawnSync(process.execPath, [checker, '--version', '12.7'], {
    input,
    encoding: 'utf8',
  })
  const healthy = run('log line\nNEURAL_DX_SELF_TEST={"ok": true, "missing": [], "version": "12.7"}\n')
  assert.equal(healthy.status, 0, healthy.stderr)

  for (const bad of [
    'NEURAL_DX_SELF_TEST={"ok": false, "missing": ["templates/index.html"], "version": "12.7"}\n',
    'NEURAL_DX_SELF_TEST={"ok": true, "missing": [], "version": "12.6"}\n',
    'unrelated output\n',
  ]) {
    assert.notEqual(run(bad).status, 0, `accepted invalid self-test payload: ${bad}`)
  }
})

test('public release cannot inherit the private-build escape hatch', () => {
  const release = read('.github/workflows/release.yml')
  assert.ok(!release.includes('NEURAL_DX_PRIVATE_BUILD'), 'release.yml bypasses the redistribution gate')
  assert.ok(release.includes('node scripts/check-neural-dx-redistribution.mjs'))
  assert.ok(release.includes('needs: [release-prerequisites, neural-dx-license]'))
})
