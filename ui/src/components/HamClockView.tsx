import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  askLog,
  deleteQsoById,
  editQsoById,
  getAllSpots,
  logQso,
  openHamClockStatus,
  pointRotator,
  postSpot,
  purgeLog,
  readRotator,
  restartOpenHamClock,
  setFrequency,
  setPtt,
  stopRotator,
  type OpenHamClockStatus,
  type QsoEdit,
} from '../api'
import { bandLabelForMhz } from '../band'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'
import { t } from '../i18n'
import type { AppSnapshot, DecodeRow, LoggedQso, SpotRow } from '../types'

const POLL_MS = 500
const DATA_POLL_MS = 2_000
const PAGE_SIZE = 500

type OpenHamQso = Record<string, unknown> & { id?: string; call?: string }
type BridgeMessage = { source?: string; type?: string; payload?: Record<string, unknown> | OpenHamQso[] }

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

function fromOpenHam(qso: OpenHamQso): LoggedQso {
  return {
    call: String(qso.call ?? '').trim().toUpperCase(),
    grid: asText(qso.gridsquare),
    state: asText(qso.state),
    band: String(qso.band ?? '').trim(),
    freqMhz: Number(qso.freq) || 0,
    mode: String(qso.submode ?? qso.mode ?? '').trim().toUpperCase(),
    rstSent: asText(qso.rst_sent),
    rstRcvd: asText(qso.rst_rcvd),
    name: asText(qso.name),
    qth: asText(qso.qth),
    comment: asText(qso.comment),
    notes: null,
    txPower: Number.isFinite(Number(qso.tx_pwr)) ? Number(qso.tx_pwr) : null,
    whenUnix: qsoTime(qso),
    timeKnown: true,
    confirmed: false,
    awardConfirmed: false,
    myGrid: asText(qso.my_gridsquare),
  }
}

function toOpenHam(row: LoggedQso, editKey: string): OpenHamQso {
  const date = new Date(row.whenUnix * 1000)
  const part = (value: number) => String(value).padStart(2, '0')
  return {
    id: row.id ?? undefined,
    call: row.call,
    qso_date: `${date.getUTCFullYear()}${part(date.getUTCMonth() + 1)}${part(date.getUTCDate())}`,
    time_on: `${part(date.getUTCHours())}${part(date.getUTCMinutes())}${part(date.getUTCSeconds())}`,
    band: row.band,
    mode: row.mode,
    freq: row.freqMhz,
    rst_sent: row.rstSent ?? '',
    rst_rcvd: row.rstRcvd ?? '',
    gridsquare: row.grid ?? '',
    state: row.state ?? '',
    name: row.name ?? '',
    qth: row.qth ?? '',
    comment: row.comment ?? '',
    tx_pwr: row.txPower ?? '',
    my_gridsquare: row.myGrid ?? '',
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

async function fullLogbook(): Promise<{ rows: OpenHamQso[]; source: Map<string, LoggedQso> }> {
  const rows: OpenHamQso[] = []
  const source = new Map<string, LoggedQso>()
  let offset = 0
  while (true) {
    const page = await askLog({ kind: 'page', query: DEFAULT_LOG_QUERY, offset, limit: PAGE_SIZE })
    page.rows.forEach((row, index) => {
      rows.push(toOpenHam(row, page.editKeys[index] ?? ''))
      if (row.id) source.set(row.id, row)
    })
    offset += page.rows.length
    if (offset >= page.total || page.rows.length === 0) break
  }
  return { rows, source }
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
    notes: old.notes ?? null,
    txPower: next.txPower ?? null,
    whenUnix: next.whenUnix,
    timeOffUnix: old.timeOffUnix ?? null,
    ota: {
      myProgram: old.ota?.myProgram ?? null,
      myRef: old.ota?.myRef ?? null,
      theirProgram: old.ota?.theirProgram ?? null,
      theirRef: old.ota?.theirRef ?? null,
    },
    myGrid: next.myGrid ?? null,
    myRig: old.myRig ?? null,
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
        logbook: logbook.current,
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

  const refreshLogbook = useCallback(async () => {
    const data = await fullLogbook()
    logbook.current = data.rows
    nexusRows.current = data.source
    sendState()
  }, [sendState])

  useEffect(() => {
    if (!origin) return
    let live = true
    fullLogbook().then((data) => {
      if (!live) return
      logbook.current = data.rows
      nexusRows.current = data.source
      sendState()
    }).catch((e) => setError(String(e)))
    return () => { live = false }
  }, [origin, snap.logTick, frameLoaded, sendState])

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
      const payload = (event.data.payload ?? {}) as OpenHamQso
      try {
        if (type === 'ready') sendState()
        else if (type === 'logbook:add') onSnap(await logQso(fromOpenHam(payload)))
        else if (type === 'logbook:addMany' && Array.isArray(event.data.payload)) {
          for (const qso of event.data.payload) onSnap(await logQso(fromOpenHam(qso)))
        } else if (type === 'logbook:update' && payload.id) {
          const old = nexusRows.current.get(payload.id)
          const editKey = asText(payload._nexusEditKey)
          if (!old || !editKey) throw new Error('Nexus could not identify the logbook row to update')
          const answer = await editQsoById({ id: payload.id, editKey }, editFromOpenHam(payload, old))
          if (answer.kind !== 'applied') throw new Error(`Nexus rejected the logbook update (${answer.kind})`)
          await refreshLogbook()
        } else if (type === 'logbook:remove' && payload.id) {
          const mirrored = logbook.current.find((qso) => qso.id === payload.id)
          const editKey = asText(mirrored?._nexusEditKey)
          if (!editKey) throw new Error('Nexus could not identify the logbook row to delete')
          const answer = await deleteQsoById({ id: payload.id, editKey })
          if (answer.kind !== 'deleted' && answer.kind !== 'gone') {
            throw new Error(`Nexus rejected the logbook delete (${answer.kind})`)
          }
          await refreshLogbook()
        } else if (type === 'logbook:clear') {
          await purgeLog()
          await refreshLogbook()
        }
        else if (type === 'rig:setFreq') {
          const mhz = Number(payload.freq) / 1_000_000
          if (mhz > 0) onSnap(await setFrequency(mhz, bandLabelForMhz(mhz), snap.radio.sideband))
        } else if (type === 'rig:setMode') {
          const mode = asText(payload.mode)
          if (mode) onSnap(await setFrequency(snap.radio.dialMhz, snap.radio.band, mode))
        } else if (type === 'rig:setPTT') onSnap(await setPtt(payload.ptt === true))
        else if (type === 'rotator:turn') await pointRotator(Number(payload.azimuth))
        else if (type === 'rotator:stop') await stopRotator()
        else if (type === 'cluster:post') {
          const call = asText(payload.call)
          const freqKhz = Number(payload.freqKhz)
          if (call && freqKhz > 0) await postSpot(freqKhz / 1000, call, asText(payload.comment) ?? '')
        }
      } catch (e) {
        setError(String(e))
      }
    }
    window.addEventListener('message', receive)
    return () => window.removeEventListener('message', receive)
  }, [onSnap, origin, refreshLogbook, sendState, snap.radio])

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
