import { createHash } from 'node:crypto'
import { readFileSync, statSync } from 'node:fs'
import { basename } from 'node:path'

export const BUILD_RECEIPT_SCHEMA = 1

export function sha256Bytes(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

export function sha256File(path) {
  return sha256Bytes(readFileSync(path))
}

export function executableNameFor(target) {
  return target === 'win-x64' ? 'openhamclock.exe' : 'openhamclock'
}

export function executableFormat(path, target) {
  const header = readFileSync(path).subarray(0, 4096)
  if (target === 'win-x64') {
    const peOffset = header.length >= 0x40 ? header.readUInt32LE(0x3c) : -1
    if (header.subarray(0, 2).toString('ascii') !== 'MZ' || peOffset < 0 || peOffset + 6 > header.length ||
        header.subarray(peOffset, peOffset + 4).toString('binary') !== 'PE\0\0' || header.readUInt16LE(peOffset + 4) !== 0x8664) {
      throw new Error('OpenHamClock staged executable is not a Windows x86-64 PE binary')
    }
    return 'PE32+ x86-64'
  }
  if (target.startsWith('linux-')) {
    const expectedMachine = target === 'linux-arm64' ? 0xb7 : 0x3e
    if (!header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) || header[4] !== 2 ||
        header[5] !== 1 || header.readUInt16LE(18) !== expectedMachine) {
      throw new Error(`OpenHamClock staged executable is not a ${target} ELF binary`)
    }
    return target === 'linux-arm64' ? 'ELF64 little-endian aarch64' : 'ELF64 little-endian x86-64'
  }
  if (target.startsWith('macos-')) {
    if (header.readUInt32LE(0) !== 0xfeedfacf) throw new Error('OpenHamClock staged executable is not a macOS Mach-O binary')
    const cpu = header.readUInt32LE(4)
    const expectedCpu = target === 'macos-arm64' ? 0x0100000c : 0x01000007
    if (cpu !== expectedCpu) throw new Error(`OpenHamClock staged executable architecture does not match ${target}`)
    return target === 'macos-arm64' ? 'Mach-O 64-bit arm64' : 'Mach-O 64-bit x86_64'
  }
  throw new Error(`Unsupported OpenHamClock target: ${target}`)
}

function receiptPayload(receipt) {
  return {
    schemaVersion: receipt.schemaVersion,
    generatedAt: receipt.generatedAt,
    source: receipt.source,
    target: receipt.target,
    executable: receipt.executable,
  }
}

export function receiptId(receipt) {
  return sha256Bytes(Buffer.from(JSON.stringify(receiptPayload(receipt))))
}

export function createBuildReceipt({ executable, target, manifest, generatedAt = new Date().toISOString() }) {
  const receipt = {
    schemaVersion: BUILD_RECEIPT_SCHEMA,
    generatedAt,
    source: { repository: manifest.repository, commit: manifest.commit, version: manifest.version },
    target,
    executable: {
      name: basename(executable),
      size: statSync(executable).size,
      sha256: sha256File(executable),
      format: executableFormat(executable, target),
    },
  }
  return { ...receipt, receiptId: receiptId(receipt) }
}

export function validateBuildReceipt({ receipt, executable, target, expected }) {
  if (!receipt || receipt.schemaVersion !== BUILD_RECEIPT_SCHEMA) throw new Error('OpenHamClock build receipt is missing or unsupported')
  if (receipt.receiptId !== receiptId(receipt)) throw new Error('OpenHamClock build receipt digest mismatch')
  if (receipt.target !== target || receipt.source?.commit !== expected.commit || receipt.source?.version !== expected.version ||
      receipt.source?.repository !== expected.repository) {
    throw new Error('OpenHamClock build receipt source or target is stale')
  }
  const actual = {
    name: basename(executable),
    size: statSync(executable).size,
    sha256: sha256File(executable),
    format: executableFormat(executable, target),
  }
  if (JSON.stringify(receipt.executable) !== JSON.stringify(actual)) {
    throw new Error('OpenHamClock executable does not match its same-run build receipt')
  }
  return { receiptId: receipt.receiptId, ...actual }
}
