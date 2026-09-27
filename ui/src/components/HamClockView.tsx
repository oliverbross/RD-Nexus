import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  askLog,
  deleteQsoById,
  editQsoById,
  exportLog,
  getAllSpots,
  logQso,
  openHamClockLogDelta,
  openHamClockStatus,
  pointRotator,
  postSpot,
  purgeLog,
  readRotator,
  restartOpenHamClock,
  saveTextToDownloads,
  setFrequency,
  setOpenHamClockPtt,
  stopRotator,
  type OpenHamClockStatus,
  type QsoEdit,
} from '../api'
import { bandLabelForMhz } from '../band'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'
import { t } from '../i18n'
import { confirmDialog } from '../confirm'
import type { AppSnapshot, DecodeRow, LoggedQso, SpotRow } from '../types'

const POLL_MS = 500
const DATA_POLL_MS = 2_000
const PAGE_SIZE = 500
export const MAX_LOGBOOK_CONTACTS = 1_000_000
export const MAX_LOGBOOK_PAGES = Math.ceil(MAX_LOGBOOK_CONTACTS / PAGE_SIZE)
const OPENHAMCLOCK_ID_FIELD = 'APP_OPENHAMCLOCK_ID'
export const LOGBOOK_CHUNK_ROWS = 500
export const LOGBOOK_CHUNK_BYTES = 512 * 1024
export const LOGBOOK_ROW_BYTES = 1024 * 1024

type OpenHamQso = Record<string, unknown> & { id?: string; call?: string }
type BridgeMessage = { source?: string; type?: string; payload?: Record<string, unknown> | OpenHamQso[] }

export function chunkOpenHamLogbook(rows: OpenHamQso[]): OpenHamQso[][] {
  const encoder = new TextEncoder()
  const chunks: OpenHamQso[][] = []
  let chunk: OpenHamQso[] = []
  let bytes = 2
  for (const row of rows) {
    const rowBytes = encoder.encode(JSON.stringify(row)).length + 1
    if (rowBytes > LOGBOOK_ROW_BYTES) throw new Error('An OpenHamClock logbook row exceeds the 1 MiB bridge safety limit')
    if (chunk.length > 0 && (chunk.length >= LOGBOOK_CHUNK_ROWS || bytes + rowBytes > LOGBOOK_CHUNK_BYTES)) {
      chunks.push(chunk)
      chunk = []
      bytes = 2
    }
    chunk.push(row)
    bytes += rowBytes
  }
  if (chunk.length > 0) chunks.push(chunk)
  return chunks
}

function asText(value: unknown): string | null {
  const text = String(value ?? '').trim()
  return text || null
}

function qsoTime(qso: OpenHamQso): number {
  const date = String(qso.qso_date ?? '').replace(/\D/g, '')
  const time = String(qso.time_on ?? '').replace(/\D/g, '').padEnd(6, '0')
  if (date.length !== 8) return Math.floor(Date.now() / 1000)
  return Math.floor(Date.UTC(
    Number(date.slice(0, 4)), Number(date.slice(4, 6)) - 1, Number(date.slice(6, 8)),
    Number(time.slice(0, 2)), Number(time.slice(2, 4)), Number(time.slice(4, 6)),
  ) / 1000)
}

export function fromOpenHam(qso: OpenHamQso): LoggedQso {
  const extraObject = qso.extras && typeof qso.extras === 'object' && !Array.isArray(qso.extras)
    ? qso.extras as Record<string, unknown>
    : {}
  const extras = Object.entries(extraObject)
    .map(([key, value]) => [key.toUpperCase(), String(value)] as [string, string])
  const field = (name: string): unknown => qso[name.toLowerCase()] ?? extraObject[name.toUpperCase()]
  const text = (name: string): string | null => asText(field(name))
  const yes = (name: string): boolean => String(field(name) ?? '').toUpperCase() === 'Y'
  if (qso.id && !extras.some(([key]) => key === OPENHAMCLOCK_ID_FIELD)) {
    extras.push([OPENHAMCLOCK_ID_FIELD, qso.id])
  }
  return {
    call: String(qso.call ?? '').trim().toUpperCase(),
    grid: asText(qso.gridsquare),
    country: text('COUNTRY'),
    state: text('STATE'),
    band: String(qso.band ?? '').trim(),
    freqMhz: Number(qso.freq) || 0,
    freqRxMhz: Number.isFinite(Number(field('FREQ_RX'))) ? Number(field('FREQ_RX')) : null,
    mode: String(qso.submode ?? qso.mode ?? '').trim().toUpperCase(),
    rstSent: asText(qso.rst_sent),
    rstRcvd: asText(qso.rst_rcvd),
    name: asText(qso.name),
    qth: text('QTH'),
    comment: asText(qso.comment),
    notes: text('NOTES'),
    txPower: Number.isFinite(Number(qso.tx_pwr)) ? Number(qso.tx_pwr) : null,
    whenUnix: qsoTime(qso),
    timeKnown: true,
    timeOffUnix: field('QSO_DATE_OFF') || field('TIME_OFF') ? qsoTime({ qso_date: field('QSO_DATE_OFF') ?? qso.qso_date, time_on: field('TIME_OFF') ?? '000000' }) : null,
    confirmed: yes('QSL_RCVD') || yes('LOTW_QSL_RCVD') || yes('EQSL_QSL_RCVD'),
    awardConfirmed: yes('QSL_RCVD') || yes('LOTW_QSL_RCVD'),
    qslRcvd: {
      card: yes('QSL_RCVD'),
      lotw: yes('LOTW_QSL_RCVD'),
      eqsl: yes('EQSL_QSL_RCVD'),
      qrz: yes('QRZCOM_QSO_UPLOAD_STATUS'),
    },
    qslSent: {
      sent: yes('QSL_SENT'),
      via: text('QSL_SENT_VIA'),
      dateUnix: null,
    },
    ota: {
      myProgram: text('MY_SIG'),
      myRef: text('MY_SIG_INFO'),
      theirProgram: text('SIG'),
      theirRef: text('SIG_INFO'),
      iota: text('IOTA'),
    },
    dxcc: Number.isFinite(Number(field('DXCC'))) ? Number(field('DXCC')) : null,
    propMode: text('PROP_MODE'),
    satName: text('SAT_NAME'),
    operator: text('OPERATOR'),
    stationCallsign: text('STATION_CALLSIGN'),
    myGrid: text('MY_GRIDSQUARE'),
    myRig: text('MY_RIG'),
    extra: extras,
  }
}

export function toOpenHam(row: LoggedQso, editKey: string): OpenHamQso {
  const date = new Date(row.whenUnix * 1000)
  const part = (value: number) => String(value).padStart(2, '0')
  const off = row.timeOffUnix ? new Date(row.timeOffUnix * 1000) : null
  const boundId = row.extra?.find(([key]) => key.toUpperCase() === OPENHAMCLOCK_ID_FIELD)?.[1]
  return {
    id: boundId || row.id || undefined,
    call: row.call,
    qso_date: `${date.getUTCFullYear()}${part(date.getUTCMonth() + 1)}${part(date.getUTCDate())}`,
    time_on: `${part(date.getUTCHours())}${part(date.getUTCMinutes())}${part(date.getUTCSeconds())}`,
    qso_date_off: off ? `${off.getUTCFullYear()}${part(off.getUTCMonth() + 1)}${part(off.getUTCDate())}` : '',
    time_off: off ? `${part(off.getUTCHours())}${part(off.getUTCMinutes())}${part(off.getUTCSeconds())}` : '',
    band: row.band,
    mode: row.mode,
    freq: row.freqMhz,
    freq_rx: row.freqRxMhz ?? '',
    rst_sent: row.rstSent ?? '',
    rst_rcvd: row.rstRcvd ?? '',
    gridsquare: row.grid ?? '',
    country: row.country ?? '',
    state: row.state ?? '',
    name: row.name ?? '',
    qth: row.qth ?? '',
    comment: row.comment ?? '',
    notes: row.notes ?? '',
    tx_pwr: row.txPower ?? '',
    my_gridsquare: row.myGrid ?? '',
    my_rig: row.myRig ?? '',
    dxcc: row.dxcc ?? '',
    prop_mode: row.propMode ?? '',
    sat_name: row.satName ?? '',
    operator: row.operator ?? '',
    station_callsign: row.stationCallsign ?? '',
    my_sig: row.ota?.myProgram ?? '',
    my_sig_info: row.ota?.myRef ?? '',
    sig: row.ota?.theirProgram ?? '',
    sig_info: row.ota?.theirRef ?? '',
    iota: row.ota?.iota ?? '',
    qsl_sent: row.qslSent?.sent ? 'Y' : 'N',
    qsl_sent_via: row.qslSent?.via ?? '',
    qsl_rcvd: row.qslRcvd?.card ? 'Y' : 'N',
    lotw_qsl_rcvd: row.qslRcvd?.lotw ? 'Y' : 'N',
    eqsl_qsl_rcvd: row.qslRcvd?.eqsl ? 'Y' : 'N',
    extras: Object.fromEntries(row.extra ?? []),
    _nexusEditKey: editKey,
  }
}

function gridCenter(grid: string | null | undefined): { lat: number; lon: number } | null {
  const value = String(grid ?? '').trim().toUpperCase()
  if (!/^[A-R]{2}\d{2}([A-X]{2}(\d{2})?)?$/.test(value)) return null
  let lon = (value.charCodeAt(0) - 65) * 20 - 180 + Number(value[2]) * 2
  let lat = (value.charCodeAt(1) - 65) * 10 - 90 + Number(value[3])
  if (value.length >= 6) {
    lon += (value.charCodeAt(4) - 65) / 12
    lat += (value.charCodeAt(5) - 65) / 24
  } else {
    return { lat: lat + 0.5, lon: lon + 1 }
  }
  if (value.length >= 8) {
    lon += Number(value[6]) / 120 + 1 / 240
    lat += Number(value[7]) / 240 + 1 / 480
  } else {
    lon += 1 / 24
    lat += 1 / 48
  }
  return { lat, lon }
}

function toOpenHamSpot(spot: SpotRow, stationGrid: string) {
  const timestamp = Date.now() - Math.max(0, spot.ageSecs) * 1000
  const dx = gridCenter(spot.grid)
  const de = gridCenter(stationGrid)
  return {
    id: `${spot.call}-${spot.freqMhz}-${spot.spotter}`,
    dxCall: spot.call,
    dxGrid: spot.grid ?? null,
    dxLat: dx?.lat ?? null,
    dxLon: dx?.lon ?? null,
    spotterGrid: stationGrid || null,
    spotterLat: de?.lat ?? null,
    spotterLon: de?.lon ?? null,
    freq: spot.freqMhz.toFixed(6),
    band: spot.band,
    mode: spot.submode ?? spot.mode,
    comment: spot.comment,
    spotter: spot.spotter,
    timestamp,
    time: new Date(timestamp).toISOString().slice(11, 16) + 'z',
    source: 'Nexus',
  }
}

function decodeKey(row: DecodeRow): string {
  return `${row.from ?? ''}\u0000${row.freqHz}\u0000${row.message}`
}

function toOpenHamDecode(row: DecodeRow, snap: AppSnapshot, firstSeen: Map<string, number>) {
  const location = gridCenter(row.grid)
  const key = decodeKey(row)
  const timestamp = row.txAt ? row.txAt * 1000 : (firstSeen.get(key) ?? Date.now())
  firstSeen.set(key, timestamp)
  const from = row.from ?? ''
  return {
    id: `nexus-${timestamp}-${row.freqHz}-${row.message}`,
    clientId: 'RD-Nexus',
    isNew: true,
    time: new Date(timestamp).toISOString().slice(11, 19),
    timeMs: timestamp,
    snr: row.snr,
    dt: row.dtSec.toFixed(1),
    freq: row.freqHz,
    mode: snap.mode,
    message: row.message,
    lowConfidence: row.lowConf ?? false,
    offAir: false,
    dialFrequency: snap.radio.dialMhz * 1_000_000,
    band: snap.radio.band,
    caller: from,
    deCall: from,
    dxCall: from,
    grid: row.grid ?? null,
    lat: location?.lat ?? null,
    lon: location?.lon ?? null,
    isCQ: row.isCq,
    directedToMe: row.directedToMe,
    timestamp,
  }
}

export async function loadOpenHamLogbook(): Promise<{ rows: OpenHamQso[]; source: Map<string, LoggedQso>; revision: number }> {
  const rows: OpenHamQso[] = []
  const source = new Map<string, LoggedQso>()
  let offset = 0
  let expectedTotal: number | null = null
  let expectedRevision: number | null = null
  for (let pageNumber = 0; pageNumber < MAX_LOGBOOK_PAGES; pageNumber += 1) {
    const page = await askLog({ kind: 'page', query: DEFAULT_LOG_QUERY, offset, limit: PAGE_SIZE })
    if (!Number.isSafeInteger(page.total) || page.total < 0 || page.total > MAX_LOGBOOK_CONTACTS) {
      throw new Error('Nexus returned an invalid or unsupported OpenHamClock logbook total')
    }
    if (!Number.isSafeInteger(page.revision) || page.revision < 0) {
      throw new Error('Nexus returned an invalid OpenHamClock logbook revision')
    }
    if (expectedTotal == null) expectedTotal = page.total
    if (expectedRevision == null) expectedRevision = page.revision
    if (page.total !== expectedTotal || page.revision !== expectedRevision || page.offset !== offset || page.rows.length > PAGE_SIZE) {
      throw new Error('Nexus logbook changed or returned an invalid page during OpenHamClock synchronization')
    }
    if (page.rows.length === 0 && offset < page.total) {
      throw new Error('Nexus logbook paging made no progress during OpenHamClock synchronization')
    }
    page.rows.forEach((row, index) => {
      const projected = toOpenHam(row, page.editKeys[index] ?? '')
      rows.push(projected)
      if (projected.id) source.set(projected.id, row)
    })
    offset += page.rows.length
    if (offset >= page.total) return { rows, source, revision: expectedRevision }
  }
  throw new Error('Nexus logbook exceeded the bounded OpenHamClock page count')
}

function editFromOpenHam(qso: OpenHamQso, old: LoggedQso): QsoEdit {
  const next = fromOpenHam(qso)
  return {
    call: next.call || old.call,
    grid: next.grid,
    state: next.state ?? null,
    band: next.band || old.band,
    freqMhz: next.freqMhz || old.freqMhz,
    mode: next.mode || old.mode,
    rstSent: next.rstSent,
    rstRcvd: next.rstRcvd,
    name: next.name ?? null,
    qth: next.qth ?? null,
    comment: next.comment ?? null,
    notes: next.notes ?? old.notes ?? null,
    txPower: next.txPower ?? null,
    whenUnix: next.whenUnix,
    timeOffUnix: old.timeOffUnix ?? null,
    ota: {
      myProgram: next.ota?.myProgram ?? old.ota?.myProgram ?? null,
      myRef: next.ota?.myRef ?? old.ota?.myRef ?? null,
      theirProgram: next.ota?.theirProgram ?? old.ota?.theirProgram ?? null,
      theirRef: next.ota?.theirRef ?? old.ota?.theirRef ?? null,
    },
    myGrid: next.myGrid ?? null,
    myRig: next.myRig ?? old.myRig ?? null,
    qslSentVia: old.qslSent?.sent ? ((old.qslSent.via as 'B' | 'D' | 'E' | null) ?? 'SENT') : null,
    qslCard: old.qslRcvd?.card ?? false,
  }
}

export function HamClockView({ snap, onSnap }: { snap: AppSnapshot; onSnap: (snap: AppSnapshot) => void }) {
  const [status, setStatus] = useState<OpenHamClockStatus | null>(null)
  const [error, setError] = useState('')
  const [frameLoaded, setFrameLoaded] = useState(0)
  const frame = useRef<HTMLIFrameElement>(null)
  const snapRef = useRef(snap)
  snapRef.current = snap
  const logbook = useRef<OpenHamQso[]>([])
  const logRevision = useRef(0)
  const nativeLogRevision = useRef<number | null>(null)
  const observedLogTick = useRef(snap.logTick)
  const logSyncQueue = useRef<Promise<void>>(Promise.resolve())
  const mirrorAcks = useRef(new Map<number, {
    resolve: (value: { status: string; revision: number }) => void
    reject: (reason: Error) => void
    timer: number
  }>())
  const nexusRows = useRef(new Map<string, LoggedQso>())
  const decodeTimes = useRef(new Map<string, number>())
  const spots = useRef<ReturnType<typeof toOpenHamSpot>[]>([])
  const rotator = useRef<{ azimuth: number | null; live: boolean }>({ azimuth: null, live: false })

  const refresh = useCallback(async () => {
    try {
      setStatus(await openHamClockStatus())
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }, [])

  useEffect(() => {
    void refresh()
    if (status?.phase === 'ready') return
    const timer = window.setInterval(() => void refresh(), POLL_MS)
    return () => window.clearInterval(timer)
  }, [refresh, status?.phase])

  const origin = useMemo(() => status?.url ? new URL(status.url).origin : '', [status?.url])
  const sendState = useCallback(() => {
    const current = snapRef.current
    const recentDecodes = current.recentDecodes ?? []
    const activeDecodeKeys = new Set(recentDecodes.map(decodeKey))
    for (const key of decodeTimes.current.keys()) {
      if (!activeDecodeKeys.has(key)) decodeTimes.current.delete(key)
    }
    frame.current?.contentWindow?.postMessage({
      source: 'nexus',
      type: 'state',
      payload: {
        station: { callsign: current.mycall, grid: current.mygrid },
        rig: {
          connected: current.radio.catOk !== false,
          freq: current.radio.dialMhz * 1_000_000,
          mode: current.radio.sideband,
          ptt: current.radio.transmitting,
        },
        spots: spots.current,
        rotator: rotator.current,
        wsjtx: {
          decodes: recentDecodes.map((row) => toOpenHamDecode(row, current, decodeTimes.current)),
          client: {
            dialFrequency: current.radio.dialMhz * 1_000_000,
            mode: current.mode,
            band: current.radio.band,
            transmitting: current.radio.transmitting,
            decoding: !current.radio.transmitting,
            lastSeen: Date.now(),
          },
        },
      },
    }, origin)
  }, [origin])

  const enqueueLogSync = useCallback(<T,>(work: () => Promise<T>): Promise<T> => {
    const next = logSyncQueue.current.then(work, work)
    logSyncQueue.current = next.then(() => undefined, () => undefined)
    return next
  }, [])

  const postAndWaitForMirror = useCallback((messageRevision: number, post: () => void) => new Promise<{ status: string; revision: number }>((resolve, reject) => {
    const timer = window.setTimeout(() => {
      mirrorAcks.current.delete(messageRevision)
      reject(new Error('OpenHamClock logbook mirror did not acknowledge the commit'))
    }, 30_000)
    mirrorAcks.current.set(messageRevision, { resolve, reject, timer })
    post()
  }), [])

  const sendLogbook = useCallback(async (rows: OpenHamQso[], sourceRevision: number) => {
    const target = frame.current?.contentWindow
    if (!target || !origin) throw new Error('OpenHamClock frame is not ready for logbook sync')
    const messageRevision = ++logRevision.current
    const chunks = chunkOpenHamLogbook(rows)
    const ack = await postAndWaitForMirror(messageRevision, () => {
      target.postMessage({ source: 'nexus', type: 'logbook:begin', payload: { revision: messageRevision, nativeRevision: sourceRevision, total: rows.length } }, origin)
      let offset = 0
      for (const chunk of chunks) {
        target.postMessage({ source: 'nexus', type: 'logbook:chunk', payload: { revision: messageRevision, offset, rows: chunk } }, origin)
        offset += chunk.length
      }
      target.postMessage({ source: 'nexus', type: 'logbook:end', payload: { revision: messageRevision, total: rows.length } }, origin)
    })
    if (ack.status === 'failed') throw new Error('OpenHamClock rejected the logbook snapshot')
    if (ack.revision < sourceRevision) throw new Error('OpenHamClock acknowledged an older logbook revision')
  }, [origin, postAndWaitForMirror])

  const refreshLogbookNow = useCallback(async () => {
    const data = await loadOpenHamLogbook()
    sendState()
    await sendLogbook(data.rows, data.revision)
    logbook.current = data.rows
    nexusRows.current = data.source
    nativeLogRevision.current = data.revision
    observedLogTick.current = snapRef.current.logTick
  }, [sendLogbook, sendState])

  const refreshLogbook = useCallback(() => enqueueLogSync(refreshLogbookNow), [enqueueLogSync, refreshLogbookNow])

  const syncLogDeltaNow = useCallback(async () => {
    const since = nativeLogRevision.current
    if (since == null) return refreshLogbookNow()
    const delta = await openHamClockLogDelta(since)
    if (delta.reset) return refreshLogbookNow()

    const nextRows = new Map(nexusRows.current)
    const deletedProjected = new Set<string>()
    for (const canonicalId of delta.deleted) {
      for (const [projectedId, row] of nextRows) {
        if (row.id === canonicalId) {
          deletedProjected.add(projectedId)
          nextRows.delete(projectedId)
        }
      }
    }
    const upserts = delta.upserts.map(({ row, editKey }) => {
      for (const [projectedId, current] of nextRows) {
        if (current.id === row.id) {
          deletedProjected.add(projectedId)
          nextRows.delete(projectedId)
        }
      }
      const projected = toOpenHam(row, editKey)
      if (projected.id) nextRows.set(projected.id, row)
      return projected
    })
    const byId = new Map(logbook.current.map((row) => [row.id, row]))
    for (const id of deletedProjected) byId.delete(id)
    for (const row of upserts) if (row.id) byId.set(row.id, row)
    const target = frame.current?.contentWindow
    if (!target || !origin) throw new Error('OpenHamClock frame is not ready for logbook delta sync')
    const messageRevision = ++logRevision.current
    const ack = await postAndWaitForMirror(messageRevision, () => {
      target.postMessage({
        source: 'nexus', type: 'logbook:delta',
        payload: { messageRevision, baseRevision: since, revision: delta.revision, upserts, deleted: [...deletedProjected] },
      }, origin)
    })
    if (ack.status === 'reset') return refreshLogbookNow()
    if (ack.status === 'failed') throw new Error('OpenHamClock rejected the logbook delta')
    if (ack.revision < delta.revision) throw new Error('OpenHamClock acknowledged an older logbook revision')
    nexusRows.current = nextRows
    logbook.current = [...byId.values()]
    nativeLogRevision.current = delta.revision
  }, [origin, postAndWaitForMirror, refreshLogbookNow])

  const syncLogDelta = useCallback(() => enqueueLogSync(syncLogDeltaNow), [enqueueLogSync, syncLogDeltaNow])

  useEffect(() => {
    if (!origin) return
    let live = true
    refreshLogbook().catch((e) => { if (live) setError(String(e)) })
    return () => { live = false }
  }, [origin, frameLoaded, refreshLogbook])

  useEffect(() => {
    if (!origin || nativeLogRevision.current == null || observedLogTick.current === snap.logTick) return
    observedLogTick.current = snap.logTick
    void syncLogDelta().catch((e) => setError(String(e)))
  }, [origin, snap.logTick, syncLogDelta])

  useEffect(() => {
    if (!origin) return
    let live = true
    const load = () => Promise.allSettled([getAllSpots(), readRotator()]).then(([nextSpots, azimuth]) => {
      if (!live) return
      if (nextSpots.status === 'fulfilled') {
        spots.current = nextSpots.value.map((spot) => toOpenHamSpot(spot, snapRef.current.mygrid))
      }
      if (azimuth.status === 'fulfilled') {
        rotator.current = { azimuth: azimuth.value, live: azimuth.value != null }
      } else {
        rotator.current = { azimuth: null, live: false }
      }
      sendState()
    })
    void load()
    const timer = window.setInterval(() => void load(), DATA_POLL_MS)
    return () => { live = false; window.clearInterval(timer) }
  }, [origin, frameLoaded, sendState])

  useEffect(() => {
    if (origin) sendState()
  }, [origin, snap.radio, frameLoaded, sendState])

  useEffect(() => {
    if (!origin) return
    const receive = async (event: MessageEvent<BridgeMessage>) => {
      if (event.origin !== origin || event.source !== frame.current?.contentWindow || event.data?.source !== 'openhamclock') return
      const type = event.data.type ?? ''
      if (type === 'logbook:commit') {
        const ack = event.data.payload as { messageRevision?: number; status?: string; revision?: number; error?: string }
        const waiter = mirrorAcks.current.get(Number(ack.messageRevision))
        if (!waiter) return
        mirrorAcks.current.delete(Number(ack.messageRevision))
        window.clearTimeout(waiter.timer)
        if (ack.status === 'failed') waiter.reject(new Error(ack.error || 'OpenHamClock logbook commit failed'))
        else waiter.resolve({ status: ack.status || 'failed', revision: Number(ack.revision) || 0 })
        return
      }
      const payload = (event.data.payload ?? {}) as OpenHamQso
      try {
        if (type === 'ready') { sendState(); await refreshLogbook() }
        else if (type === 'logbook:add') {
          if (!payload.id || !nexusRows.current.has(payload.id)) {
            onSnap(await logQso(fromOpenHam(payload)))
            await syncLogDelta()
          }
        }
        else if (type === 'logbook:addMany' && Array.isArray(event.data.payload)) {
          const seen = new Set(nexusRows.current.keys())
          const failures: string[] = []
          for (const qso of event.data.payload) {
            const next = fromOpenHam(qso)
            if (qso.id && seen.has(qso.id)) continue
            if (qso.id) seen.add(qso.id)
            try { onSnap(await logQso(next)) } catch (e) { failures.push(`${next.call}: ${String(e)}`) }
          }
          await syncLogDelta()
          if (failures.length > 0) throw new Error(`Nexus imported part of the OpenHamClock batch; ${failures.length} contact(s) failed: ${failures.join('; ')}`)
        } else if (type === 'logbook:update' && payload.id) {
          const old = nexusRows.current.get(payload.id)
          const editKey = asText(payload._nexusEditKey)
          if (!old?.id || !editKey) throw new Error('Nexus could not identify the logbook row to update')
          const answer = await editQsoById({ id: old.id, editKey }, editFromOpenHam(payload, old))
          if (answer.kind !== 'applied') {
            await syncLogDelta()
            throw new Error(`Nexus rejected the logbook update (${answer.kind}); the canonical row was refreshed`)
          }
          await syncLogDelta()
        } else if (type === 'logbook:remove' && payload.id) {
          const old = nexusRows.current.get(payload.id)
          const mirrored = logbook.current.find((qso) => qso.id === payload.id)
          const editKey = asText(mirrored?._nexusEditKey)
          if (!old?.id || !editKey) throw new Error('Nexus could not identify the logbook row to delete')
          const answer = await deleteQsoById({ id: old.id, editKey })
          if (answer.kind !== 'deleted' && answer.kind !== 'gone') {
            await syncLogDelta()
            throw new Error(`Nexus rejected the logbook delete (${answer.kind}); the canonical row was refreshed`)
          }
          await syncLogDelta()
        } else if (type === 'logbook:clear') {
          const approved = await confirmDialog({
            title: t('hamClock.clearConfirm'),
            body: t('hamClock.clearWarning'),
            confirmLabel: t('hamClock.clear'),
            danger: true,
          })
          if (approved) {
            const backup = await exportLog('adif')
            await saveTextToDownloads(`nexus-before-openhamclock-clear-${new Date().toISOString().slice(0, 10)}.adi`, backup)
            await purgeLog()
            await syncLogDelta()
          }
        }
        else if (type === 'rig:setFreq') {
          const mhz = Number(payload.freq) / 1_000_000
          if (mhz > 0) onSnap(await setFrequency(mhz, bandLabelForMhz(mhz), snap.radio.sideband))
        } else if (type === 'rig:setMode') {
          const mode = asText(payload.mode)
          if (mode) onSnap(await setFrequency(snap.radio.dialMhz, snap.radio.band, mode))
        } else if (type === 'rig:setPTT') onSnap(await setOpenHamClockPtt(payload.ptt === true))
        else if (type === 'rotator:turn') await pointRotator(Number(payload.azimuth))
        else if (type === 'rotator:stop') await stopRotator()
        else if (type === 'cluster:post') {
          const call = asText(payload.call)
          const freqKhz = Number(payload.freqKhz)
          if (call && freqKhz > 0) await postSpot(freqKhz / 1000, call, asText(payload.comment) ?? '')
        }
      } catch (e) {
        setError(String(e))
        frame.current?.contentWindow?.postMessage({
          source: 'nexus', type: 'mutation-result', payload: { ok: false, error: String(e) },
        }, origin)
      }
    }
    window.addEventListener('message', receive)
    return () => window.removeEventListener('message', receive)
  }, [onSnap, origin, refreshLogbook, sendLogbook, sendState, snap.radio, syncLogDelta])

  useEffect(() => () => {
    for (const waiter of mirrorAcks.current.values()) {
      window.clearTimeout(waiter.timer)
      waiter.reject(new Error('OpenHamClock view closed before logbook commit'))
    }
    mirrorAcks.current.clear()
  }, [])

  const retry = async () => {
    setError('')
    setStatus((old) => old ? { ...old, phase: 'starting', detail: null } : old)
    try {
      await restartOpenHamClock()
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  if (status?.phase === 'ready' && status.url) {
    return (
      <main className="layout single ham-clock-view">
        {error && <div className="ham-clock-error" role="alert">{error}</div>}
        {snap.radio.transmitting && (
          <button
            type="button"
            className="ham-clock-stop"
            onClick={() => void setOpenHamClockPtt(false).then(onSnap).catch((e) => setError(String(e)))}
          >
            {t('hamClock.stopTx')}
          </button>
        )}
        <iframe
          ref={frame}
          className="ham-clock-frame"
          src={`${status.url}/?nexus=1`}
          title={t('hamClock.frameTitle')}
          allow="geolocation; clipboard-read; clipboard-write"
          onLoad={() => setFrameLoaded((count) => count + 1)}
        />
      </main>
    )
  }

  const detail = error || status?.detail
  return (
    <main className="layout single ham-clock-view">
      <section className="ham-clock-state" role="status">
        <h1>{t('hamClock.title')}</h1>
        <p>{status?.phase === 'unavailable' ? t('hamClock.unavailable') : t('hamClock.starting')}</p>
        {detail && <p className="ham-clock-detail">{detail}</p>}
        {status?.phase === 'unavailable' && (
          <button type="button" className="btn secondary" onClick={() => void retry()}>
            {t('hamClock.retry')}
          </button>
        )}
      </section>
    </main>
  )
}
