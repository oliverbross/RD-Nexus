#!/usr/bin/env node

import '../integrations/openhamclock/node_modules/fake-indexeddb/auto/index.mjs'
import assert from 'node:assert/strict'
import { commitNexusDelta, commitNexusSnapshot, loadNexusMirror } from '../integrations/openhamclock/nexusMirrorStore.mjs'

const request = indexedDB.deleteDatabase('openhamclock-logbook')
await new Promise((resolve, reject) => {
  request.onsuccess = resolve
  request.onerror = () => reject(request.error)
})

async function qsos() {
  const open = indexedDB.open('openhamclock-logbook', 2)
  const db = await new Promise((resolve, reject) => {
    open.onsuccess = () => resolve(open.result)
    open.onerror = () => reject(open.error)
  })
  try {
    return await new Promise((resolve, reject) => {
      const read = db.transaction('qsos', 'readonly').objectStore('qsos').getAll()
      read.onsuccess = () => resolve(read.result)
      read.onerror = () => reject(read.error)
    })
  } finally {
    db.close()
  }
}

assert.deepEqual(await commitNexusSnapshot([{ id: 'old', call: 'K1OLD' }], ['old'], 1), { status: 'committed', revision: 1 })
assert.deepEqual(await loadNexusMirror(), { revision: 1, ids: ['old'] })
assert.deepEqual(await qsos(), [{ id: 'old', call: 'K1OLD' }])

await assert.rejects(commitNexusSnapshot([{ id: 'new', call: 'K1NEW', uncloneable: () => {} }], ['new'], 2))
assert.deepEqual(await loadNexusMirror(), { revision: 1, ids: ['old'] }, 'aborted full commit must keep the old ledger')
assert.deepEqual(await qsos(), [{ id: 'old', call: 'K1OLD' }], 'aborted full commit must keep the old rows')

await assert.rejects(commitNexusDelta([{ id: 'new', call: 'K1NEW', uncloneable: () => {} }], ['old'], 1, 2))
assert.deepEqual(await loadNexusMirror(), { revision: 1, ids: ['old'] }, 'aborted delta must keep the old ledger')
assert.deepEqual(await qsos(), [{ id: 'old', call: 'K1OLD' }], 'aborted delta must roll back deletes and upserts')

assert.deepEqual(await commitNexusDelta([{ id: 'new', call: 'K1NEW' }], ['old'], 0, 2), { status: 'reset', revision: 1 })
assert.deepEqual(await loadNexusMirror(), { revision: 1, ids: ['old'] }, 'a revision gap must not mutate the mirror')
assert.deepEqual(await commitNexusDelta([{ id: 'new', call: 'K1NEW' }], ['old'], 1, 2), { status: 'committed', revision: 2 })
assert.deepEqual(await loadNexusMirror(), { revision: 2, ids: ['new'] })
assert.deepEqual(await qsos(), [{ id: 'new', call: 'K1NEW' }])
assert.deepEqual(await commitNexusSnapshot([{ id: 'stale', call: 'K1STALE' }], ['stale'], 1), { status: 'ignored', revision: 2 })
assert.deepEqual(await qsos(), [{ id: 'new', call: 'K1NEW' }], 'a stale snapshot must not overwrite newer data')
process.stdout.write('OpenHamClock mirror rows, bindings, and revision commit atomically across interruption boundaries.\n')
