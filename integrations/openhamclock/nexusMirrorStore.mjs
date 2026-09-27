const DB_NAME = 'openhamclock-logbook'
const DB_VERSION = 2
const QSO_STORE = 'qsos'
const MIRROR_STORE = 'nexus-mirror'
const REVISION_KEY = 'revision'
const rowKey = (id) => `row:${id}`

function openMirrorDb() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION)
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(QSO_STORE)) {
        const qsos = db.createObjectStore(QSO_STORE, { keyPath: 'id' })
        qsos.createIndex('call', 'call', { unique: false })
        qsos.createIndex('qso_date', 'qso_date', { unique: false })
      }
      if (!db.objectStoreNames.contains(MIRROR_STORE)) db.createObjectStore(MIRROR_STORE, { keyPath: 'key' })
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error || new Error('OpenHamClock Nexus mirror DB open failed'))
  })
}

function transactAtRevision(db, write) {
  return new Promise((resolve, reject) => {
    const transaction = db.transaction([QSO_STORE, MIRROR_STORE], 'readwrite')
    const mirror = transaction.objectStore(MIRROR_STORE)
    const currentRequest = mirror.get(REVISION_KEY)
    let result
    currentRequest.onsuccess = () => {
      try {
        const current = Number(currentRequest.result?.value) || 0
        result = write(transaction, current)
      } catch (error) {
        transaction.abort()
        reject(error)
      }
    }
    currentRequest.onerror = () => reject(currentRequest.error || new Error('OpenHamClock Nexus revision read failed'))
    transaction.oncomplete = () => resolve(result)
    transaction.onerror = () => reject(transaction.error || new Error('OpenHamClock Nexus mirror transaction failed'))
    transaction.onabort = () => reject(transaction.error || new Error('OpenHamClock Nexus mirror transaction aborted'))
  })
}

export async function loadNexusMirror() {
  const db = await openMirrorDb()
  try {
    return await new Promise((resolve, reject) => {
      const transaction = db.transaction(MIRROR_STORE, 'readonly')
      const store = transaction.objectStore(MIRROR_STORE)
      const request = store.getAll()
      request.onsuccess = () => {
        const entries = Array.isArray(request.result) ? request.result : []
        resolve({
          revision: Number(entries.find((entry) => entry.key === REVISION_KEY)?.value) || 0,
          ids: entries.filter((entry) => String(entry.key).startsWith('row:')).map((entry) => String(entry.key).slice(4)),
        })
      }
      request.onerror = () => reject(request.error || new Error('OpenHamClock Nexus mirror read failed'))
    })
  } finally {
    db.close()
  }
}

/** Atomically replace the browser mirror and its binding ledger. A crash can
 * expose either the previous committed transaction or this complete one, never
 * a new ledger beside old rows (or new rows beside an old ledger). */
export async function commitNexusSnapshot(records, nexusIds, revision) {
  const db = await openMirrorDb()
  try {
    return await transactAtRevision(db, (transaction, current) => {
      if (revision <= current) return { status: 'ignored', revision: current }
      const qsos = transaction.objectStore(QSO_STORE)
      const mirror = transaction.objectStore(MIRROR_STORE)
      qsos.clear()
      mirror.clear()
      for (const record of records) qsos.put(record)
      for (const id of nexusIds) mirror.put({ key: rowKey(id) })
      mirror.put({ key: REVISION_KEY, value: revision })
      return { status: 'committed', revision }
    })
  } finally {
    db.close()
  }
}

/** Atomically apply a bounded native delta and advance the ledger revision. */
export async function commitNexusDelta(upserts, deleted, baseRevision, revision) {
  const db = await openMirrorDb()
  try {
    return await transactAtRevision(db, (transaction, current) => {
      if (revision <= current) return { status: 'ignored', revision: current }
      if (baseRevision !== current) return { status: 'reset', revision: current }
      const qsos = transaction.objectStore(QSO_STORE)
      const mirror = transaction.objectStore(MIRROR_STORE)
      for (const id of deleted) { qsos.delete(id); mirror.delete(rowKey(id)) }
      for (const record of upserts) { qsos.put(record); mirror.put({ key: rowKey(record.id) }) }
      mirror.put({ key: REVISION_KEY, value: revision })
      return { status: 'committed', revision }
    })
  } finally {
    db.close()
  }
}
