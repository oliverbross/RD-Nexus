#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const manifest = JSON.parse(readFileSync(join(repo, 'docs', 'neural-dx', 'upstream.json'), 'utf8'))
const dir = join(repo, 'src-tauri', 'resources', 'neural-dx')
const platformArg = process.argv.indexOf('--platform')
const platform = platformArg >= 0 ? process.argv[platformArg + 1] : process.platform
const executable = join(dir, platform === 'win32' ? 'neural-dx-watcher.exe' : 'neural-dx-watcher')
const provenance = join(dir, 'provenance.json')

if (!existsSync(executable) || statSync(executable).size < 1_000_000) throw new Error('Neural DX executable is not staged; run node scripts/stage-neural-dx.mjs')
if (!existsSync(provenance)) throw new Error('Neural DX provenance is missing')
const built = JSON.parse(readFileSync(provenance, 'utf8'))
if (built.commit !== manifest.commit) throw new Error(`Neural DX staged pin mismatch: ${built.commit} != ${manifest.commit}`)
if (built.platform === process.platform && built.arch === process.arch) {
  const selfTestDir = mkdtempSync(join(tmpdir(), 'rd-nexus-neural-dx-check-'))
  try {
    const output = execFileSync(executable, [], {
      cwd: selfTestDir,
      encoding: 'utf8',
      env: { ...process.env, NEURAL_DX_SELF_TEST: '1' },
      timeout: 60_000,
    })
    const line = output.split(/\r?\n/).find((candidate) => candidate.startsWith('NEURAL_DX_SELF_TEST='))
    if (!line) throw new Error('Neural DX staged runtime did not report its self-test')
    const selfTest = JSON.parse(line.slice('NEURAL_DX_SELF_TEST='.length))
    if (!selfTest.ok || selfTest.version !== manifest.version) {
      throw new Error(`Neural DX staged runtime self-test failed: ${JSON.stringify(selfTest)}`)
    }
  } finally {
    rmSync(selfTestDir, { recursive: true, force: true })
  }
}
process.stdout.write(`Neural DX staged: ${built.version} ${built.commit} ${built.platform}-${built.arch}\n`)
