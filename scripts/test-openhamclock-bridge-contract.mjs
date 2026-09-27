#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const source = readFileSync(new URL('../ui/src/components/HamClockView.tsx', import.meta.url), 'utf8')
const contract = readFileSync(new URL('../integrations/openhamclock/BRIDGE.md', import.meta.url), 'utf8')
const chunkBytes = Number(source.match(/LOGBOOK_CHUNK_BYTES = (\d+) \* 1024/)?.[1])
const overheadBytes = Number(source.match(/LOGBOOK_MESSAGE_OVERHEAD_BYTES = (\d+) \* 1024/)?.[1])
assert.equal(chunkBytes, 512)
assert.equal(overheadBytes, 2)
assert.match(contract, /512 KiB as a complete encoded bridge message/)
assert.match(contract, /single encoded row is\s+limited to 510 KiB, reserving 2 KiB/)
assert.doesNotMatch(contract, /1 MiB hard row limit/)
process.stdout.write('OpenHamClock bridge documentation matches the enforced 512 KiB message and 510 KiB row bounds.\n')
