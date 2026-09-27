#!/usr/bin/env node
// Tauri's resource glob matches the tracked README even when the actual companion is absent.
// Refuse that deceptive green build: the executable, license and exact provenance all ship.

import { createHash } from 'node:crypto'
import { existsSync, readFileSync, statSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptPath = fileURLToPath(import.meta.url)
const defaultRepo = resolve(dirname(scriptPath), '..')
const knownTargets = new Set(['win-x64', 'macos-arm64', 'macos-x64', 'linux-x64', 'linux-arm64'])

export function expectedTarget(argv = process.argv, env = process.env, platform = process.platform, arch = process.arch) {
  const at = argv.indexOf('--target')
  if (at >= 0 && argv[at + 1]) return argv[at + 1]
  if (env.OHC_EXPECTED_TARGET) return env.OHC_EXPECTED_TARGET
  const triple = env.TAURI_ENV_TARGET_TRIPLE || ''
  if (triple.includes('windows')) return 'win-x64'
  if (triple.includes('apple-darwin')) return triple.startsWith('aarch64') ? 'macos-arm64' : 'macos-x64'
  if (triple.includes('linux')) return triple.startsWith('aarch64') ? 'linux-arm64' : 'linux-x64'
  if (platform === 'win32') return 'win-x64'
  if (platform === 'darwin') return arch === 'arm64' ? 'macos-arm64' : 'macos-x64'
  return arch === 'arm64' ? 'linux-arm64' : 'linux-x64'
}

function assertExecutableFormat(executable, target) {
  const header = readFileSync(executable).subarray(0, 4096)
  if (target === 'win-x64') {
    const peOffset = header.length >= 0x40 ? header.readUInt32LE(0x3c) : -1
    if (header.subarray(0, 2).toString('ascii') !== 'MZ' || peOffset < 0 || peOffset + 6 > header.length ||
        header.subarray(peOffset, peOffset + 4).toString('binary') !== 'PE\0\0' || header.readUInt16LE(peOffset + 4) !== 0x8664) {
      throw new Error('OpenHamClock staged executable is not a Windows x86-64 PE binary')
    }
  }
  if (target.startsWith('linux-')) {
    const expectedMachine = target === 'linux-arm64' ? 0xb7 : 0x3e
    if (!header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) || header[4] !== 2 ||
        header[5] !== 1 || header.readUInt16LE(18) !== expectedMachine) {
      throw new Error(`OpenHamClock staged executable is not a ${target} ELF binary`)
    }
  }
  if (target.startsWith('macos-')) {
    if (header.readUInt32LE(0) !== 0xfeedfacf) throw new Error('OpenHamClock staged executable is not a macOS Mach-O binary')
    const cpu = header.readUInt32LE(4)
    const expectedCpu = target === 'macos-arm64' ? 0x0100000c : 0x01000007
    if (cpu !== expectedCpu) throw new Error(`OpenHamClock staged executable architecture does not match ${target}`)
  }
}

export function verifyStaged({ repo = defaultRepo, target = expectedTarget() } = {}) {
if (!knownTargets.has(target)) throw new Error(`Unsupported expected OpenHamClock target: ${target}`)
const dir = join(repo, 'src-tauri', 'resources', 'openhamclock')
const expected = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), 'utf8'))
const executable = join(dir, target === 'win-x64' ? 'openhamclock.exe' : 'openhamclock')
const wrongExecutable = join(dir, target === 'win-x64' ? 'openhamclock' : 'openhamclock.exe')
for (const name of ['.env', 'config.json', 'data', 'openhamclock.log', 'cluster-status.json']) {
  if (existsSync(join(dir, name))) throw new Error(`OpenHamClock runtime state must not be bundled: ${name}`)
}
if (!existsSync(executable) || statSync(executable).size < 10_000_000) {
  throw new Error(`OpenHamClock executable for ${target} is not staged; run node scripts/stage-openhamclock.mjs --target ${target}`)
}
if (existsSync(wrongExecutable)) throw new Error(`Wrong-platform OpenHamClock executable must not be bundled: ${basename(wrongExecutable)}`)
assertExecutableFormat(executable, target)
const license = join(dir, 'OPENHAMCLOCK-LICENSE.txt')
if (!existsSync(license) || statSync(license).size < 500) throw new Error('OpenHamClock MIT license is not staged')
const notices = join(dir, 'OPENHAMCLOCK-THIRD-PARTY-NOTICES.txt')
if (!existsSync(notices) || statSync(notices).size < 10_000) throw new Error('OpenHamClock third-party notices are not staged')
const nodeLicense = join(dir, 'NODE-RUNTIME-LICENSE.txt')
if (!existsSync(nodeLicense) || statSync(nodeLicense).size < 50_000) throw new Error('Node.js runtime license is not staged')
const nodeLicenseSha = createHash('sha256').update(readFileSync(nodeLicense)).digest('hex')
if (nodeLicenseSha !== expected.nodeRuntime.licenseSha256) throw new Error('Node.js runtime license checksum mismatch')
const provenance = JSON.parse(readFileSync(join(dir, 'provenance.json'), 'utf8'))
if (provenance.commit !== expected.commit || provenance.version !== expected.version || provenance.target !== target) {
  throw new Error('OpenHamClock staged provenance does not match the reviewed upstream pin')
}
if (provenance.executable !== basename(executable)) throw new Error('OpenHamClock staged executable name does not match provenance')
const executableSha256 = createHash('sha256').update(readFileSync(executable)).digest('hex')
if (provenance.executableSha256 !== executableSha256) throw new Error('OpenHamClock staged executable checksum mismatch')
const inventory = JSON.parse(readFileSync(join(dir, 'parity-inventory.json'), 'utf8'))
if (inventory.panels.length < 50 || inventory.settingsTabs.length < 10 || inventory.routes.length < 80 || inventory.storage.length < 50 || inventory.configuration.length < 80 || inventory.distFiles.length < 20) {
  throw new Error('OpenHamClock staged parity inventory is incomplete')
}
return { target, version: provenance.version, commit: provenance.commit, executableSha256 }
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  const result = verifyStaged()
  process.stdout.write(`OpenHamClock ${result.version} staged for ${result.target} (${result.commit})\n`)
}
