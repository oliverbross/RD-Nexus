#!/usr/bin/env node
import { existsSync, readFileSync, statSync } from 'node:fs'
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
process.stdout.write(`Neural DX staged: ${built.version} ${built.commit} ${built.platform}-${built.arch}\n`)
