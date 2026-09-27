#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const resource = join(repo, 'src-tauri', 'resources', 'openhamclock')
const targets = ['win-x64', 'linux-x64', 'linux-arm64', 'macos-x64', 'macos-arm64']
const inspected = []

for (const target of targets) {
  execFileSync(process.execPath, [join(repo, 'scripts', 'stage-openhamclock.mjs'), '--target', target], { cwd: repo, stdio: 'inherit' })
  const executable = join(resource, target.startsWith('win-') ? 'openhamclock.exe' : 'openhamclock')
  const bytes = readFileSync(executable)
  const format = execFileSync('file', ['-b', executable], { encoding: 'utf8' }).trim()
  const expected = target.startsWith('win-') ? /PE32\+.*x86-64/i
    : target === 'linux-x64' ? /ELF 64-bit.*x86-64/i
      : target === 'linux-arm64' ? /ELF 64-bit.*ARM aarch64/i
        : target === 'macos-x64' ? /Mach-O 64-bit.*x86_64/i
          : /Mach-O 64-bit.*arm64/i
  if (!expected.test(format)) throw new Error(`${target} produced an unexpected executable: ${format}`)
  inspected.push({
    target,
    format,
    size: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
    packageStatus: 'VERIFIED',
    runtimeStatus: 'EXTERNAL',
    runtimeEvidence: 'Package format and resource closure inspected; runtime is not inferred from the target string',
    runtimeReason: 'A separate SHA-bound smoke and browser receipt is required on the target operating system/architecture',
  })
}

writeFileSync(join(repo, 'integrations', 'openhamclock', 'package-inspection.json'), JSON.stringify({ generatedAt: new Date().toISOString(), targets: inspected }, null, 2) + '\n')
process.stdout.write(`Verified OpenHamClock executable resources for ${targets.join(', ')}; macos-arm64 left staged for the host build.\n`)
