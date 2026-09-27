#!/usr/bin/env node
import assert from 'node:assert/strict'
import { reconcileNexusLog } from '../integrations/openhamclock/nexusLogMerge.mjs'

const native = { id: 'native-1', call: 'N0NATIVE' }
const mirrored = { id: 'nexus-removed', call: 'N0OLD' }
const afterRestart = reconcileNexusLog([native, mirrored], ['nexus-removed'], [])
assert.deepEqual(afterRestart.records, [native], 'a persisted mirror ledger must remove a Nexus-side deletion after restart')

const localAdd = { id: 'ohc-op-1', call: 'K1ABC', comment: 'local pending' }
const canonicalReceipt = { id: 'ohc-op-1', call: 'K1ABC', comment: 'Nexus receipt', _nexusEditKey: 'receipt-1' }
const reconciled = reconcileNexusLog([localAdd], [], [canonicalReceipt])
assert.equal(reconciled.records.length, 1, 'a canonical receipt must replace, not duplicate, the native row')
assert.equal(reconciled.records[0].comment, 'Nexus receipt')

const sameMinute = reconcileNexusLog([], [], [
  { id: 'ohc-op-a', call: 'K1ABC', qso_date: '20260927', time_on: '123401', band: '20m' },
  { id: 'ohc-op-b', call: 'K1ABC', qso_date: '20260927', time_on: '123459', band: '20m' },
])
assert.equal(sameMinute.records.length, 2, 'explicit ids must preserve legitimate same-minute contacts')
process.stdout.write('OpenHamClock durable binding, restart deletion, receipt replacement, and same-minute controls pass.\n')
