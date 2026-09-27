import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { validateBuildReceipt } from './openhamclock-build-receipt.mjs'

export const REQUIRED_BROWSER_CHECKS = [
  'dashboardRendered', 'settingsRendered', 'serviceWorkerActivated', 'staleCacheEvicted', 'offlineReloadRendered', 'noPageErrors',
]

export function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function validateRuntimeReceipt({ repo, receipt, candidateSha, targetEntry }) {
  const fail = (reason) => ({ valid: false, reason })
  if (!receipt || receipt.schemaVersion !== 2) return fail('missing or unsupported runtime receipt')
  if (receipt.candidateSha !== candidateSha) return fail('runtime receipt candidate SHA is stale')
  if (receipt.target !== targetEntry?.target) return fail('runtime receipt target does not match package inspection')
  const executableName = receipt.target === 'win-x64' ? 'openhamclock.exe' : 'openhamclock'
  const executable = join(repo, 'src-tauri', 'resources', 'openhamclock', executableName)
  if (!existsSync(executable)) return fail('runtime receipt executable is absent')
  const expected = JSON.parse(readFileSync(join(repo, 'integrations', 'openhamclock', 'upstream.json'), 'utf8'))
  const buildReceipt = JSON.parse(readFileSync(join(repo, 'src-tauri', 'resources', 'openhamclock', 'build-receipt.json'), 'utf8'))
  let build
  try {
    build = validateBuildReceipt({ receipt: buildReceipt, executable, target: receipt.target, expected })
  } catch (error) {
    return fail(`runtime receipt build evidence is invalid: ${String(error)}`)
  }
  if (receipt.buildReceiptId !== build.receiptId || receipt.executableSha256 !== build.sha256 || targetEntry?.sourceCommit !== expected.commit) {
    return fail('runtime receipt executable hash or same-run build receipt is stale')
  }
  const provenance = JSON.parse(readFileSync(join(repo, 'src-tauri', 'resources', 'openhamclock', 'provenance.json'), 'utf8'))
  if (receipt.provenance?.commit !== provenance.commit || receipt.provenance?.version !== provenance.version ||
      receipt.provenance?.target !== provenance.target || provenance.target !== receipt.target) {
    return fail('runtime receipt provenance does not match the staged target')
  }
  if (receipt.smoke?.exitCode !== 0 || receipt.smoke?.passed !== true || !/^[a-f0-9]{64}$/.test(receipt.smoke?.outputSha256 || '')) {
    return fail('runtime receipt has no successful packaged smoke evidence')
  }
  if (receipt.browser?.passed !== true || REQUIRED_BROWSER_CHECKS.some((name) => receipt.browser?.checks?.[name] !== true)) {
    return fail('runtime receipt has incomplete browser evidence')
  }
  return { valid: true, reason: '', evidence: `receipt ${receipt.receiptId}; smoke ${receipt.smoke.outputSha256}; browser ${receipt.browser.evidenceSha256}` }
}

export function runtimeReceiptForTarget({ repo, integration, candidateSha, targetEntry }) {
  const path = join(integration, 'runtime-receipt.json')
  if (!existsSync(path)) return { valid: false, reason: 'No SHA-bound runtime receipt is present' }
  try {
    return validateRuntimeReceipt({ repo, receipt: JSON.parse(readFileSync(path, 'utf8')), candidateSha, targetEntry })
  } catch (error) {
    return { valid: false, reason: `Runtime receipt is unreadable: ${String(error)}` }
  }
}
