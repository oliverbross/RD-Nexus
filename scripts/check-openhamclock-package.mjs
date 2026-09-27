#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { executableNameFor, validateBuildReceipt } from './openhamclock-build-receipt.mjs'

const scriptPath = fileURLToPath(import.meta.url)
const defaultRepo = resolve(dirname(scriptPath), '..')

export function verifyPackaged({ repo = defaultRepo, bundleResources, target }) {
  const staged = join(repo, 'src-tauri', 'resources', 'openhamclock')
  const packaged = join(bundleResources, 'resources', 'openhamclock')
  const expected = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), 'utf8'))
  const executableName = executableNameFor(target)
  const stagedExecutable = join(staged, executableName)
  const packagedExecutable = join(packaged, executableName)
  const stagedReceiptBytes = readFileSync(join(staged, 'build-receipt.json'))
  const packagedReceiptBytes = readFileSync(join(packaged, 'build-receipt.json'))
  if (!stagedReceiptBytes.equals(packagedReceiptBytes)) throw new Error('Packaged OpenHamClock build receipt differs from the staged same-run receipt')
  const receipt = JSON.parse(stagedReceiptBytes)
  const stagedResult = validateBuildReceipt({ receipt, executable: stagedExecutable, target, expected })
  const packagedResult = validateBuildReceipt({ receipt, executable: packagedExecutable, target, expected })
  if (!readFileSync(stagedExecutable).equals(readFileSync(packagedExecutable)) || stagedResult.sha256 !== packagedResult.sha256) {
    throw new Error('Packaged OpenHamClock executable differs from the staged same-run artifact')
  }
  return { target, receiptId: receipt.receiptId, executableSha256: packagedResult.sha256, format: packagedResult.format }
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  const value = (name) => {
    const at = process.argv.indexOf(name)
    if (at < 0 || !process.argv[at + 1]) throw new Error(`Missing ${name}`)
    return resolve(process.argv[at + 1])
  }
  const targetAt = process.argv.indexOf('--target')
  if (targetAt < 0 || !process.argv[targetAt + 1]) throw new Error('Missing --target')
  const result = verifyPackaged({ repo: defaultRepo, bundleResources: value('--bundle-resources'), target: process.argv[targetAt + 1] })
  process.stdout.write(`Verified packaged OpenHamClock ${result.target} receipt ${result.receiptId} (${result.executableSha256})\n`)
}
