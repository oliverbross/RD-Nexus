import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import { Activity, BrainCircuit, CloudLightning, Crosshair, MonitorUp, Newspaper, Orbit, RadioTower, RotateCcw } from 'lucide-react'
import { gridToLatLon, haversineKm } from '../grid'
import { t } from '../i18n'
import { askLog, getNeuralDxActions, getNeuralDxStatus, postSpot, restartNeuralDx, syncNeuralDx, type NeuralDxStatus } from '../api'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'
import type { LoggedQso, NeedAlert, SpotRow } from '../types'
import type { View } from '../features/registry'

type NeuralTab = 'watcher' | 'hunt' | 'insight' | 'propagation' | 'weather' | 'satellites' | 'briefing'

interface Props {
  spots: SpotRow[]
  needs: NeedAlert[]
  myCall: string
  myGrid: string
  logTick: number
  selectedCall: string | null
  onSelect: (call: string) => void
  onWork: (spot: SpotRow) => void
  onNavigate: (view: View) => void
}

interface HistoryPoint {
  key: string
  band: string
  hour: number
  ts: number
}

interface HuntTarget {
  spot: SpotRow
  distanceKm: number | null
  score: number
  need: NeedAlert | null
}

interface WatcherQsoSummary {
  confirmedCalls: string[]
  confirmedDxcc: string[]
  workedCalls: string[]
  workedDxcc: string[]
  dxccByBand: Record<string, string[]>
  totalQso: number
  totalConfirmed: number
}

const EMPTY_QSO_SUMMARY: WatcherQsoSummary = {
  confirmedCalls: [], confirmedDxcc: [], workedCalls: [], workedDxcc: [],
  dxccByBand: {}, totalQso: 0, totalConfirmed: 0,
}

const HISTORY_KEY = 'nexus.neuralDx.history.v1'
const WATCH_KEY = 'nexus.neuralDx.watch.v1'
const HISTORY_MAX = 5000
const HISTORY_AGE_MS = 30 * 24 * 60 * 60 * 1000

function loadJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : fallback
  } catch {
    return fallback
  }
}

function saveJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // The live dashboard remains useful when storage is full or unavailable.
  }
}

export function scoreHuntTargets(spots: SpotRow[], needs: NeedAlert[], myGrid: string): HuntTarget[] {
  const me = gridToLatLon(myGrid)
  const needsByCall = new Map(needs.map((need) => [`${need.call.toUpperCase()}|${need.band}`, need]))
  const bestByCall = new Map<string, HuntTarget>()
  for (const spot of spots) {
    if (spot.beacon || !spot.call.trim()) continue
    const dx = spot.grid ? gridToLatLon(spot.grid) : null
    const distanceKm = me && dx ? haversineKm(me, dx) : null
    const need = needsByCall.get(`${spot.call.toUpperCase()}|${spot.band}`) ?? null
    const split = /(?:\bup\b|\bdown\b|\bqsx\b|\bsplit\b)/i.test(spot.comment)
    const fresh = Math.max(0, 30 - Math.max(0, spot.ageSecs) / 60)
    const score =
      (need?.priority ?? 0) * 100 +
      (spot.workedAgoSecs == null ? 300 : 0) +
      (distanceKm == null ? 0 : Math.min(distanceKm, 20000) / 40) +
      (split ? 35 : 0) +
      Math.min(spot.corroborators.length, 5) * 8 +
      fresh
    const target = { spot, distanceKm, score, need }
    const previous = bestByCall.get(spot.call.toUpperCase())
    if (!previous || previous.score < score) bestByCall.set(spot.call.toUpperCase(), target)
  }
  return [...bestByCall.values()].sort((a, b) => b.score - a.score || a.spot.call.localeCompare(b.spot.call))
}

function recordHistory(spots: SpotRow[], previous: HistoryPoint[]): HistoryPoint[] {
  const now = Date.now()
  const cutoff = now - HISTORY_AGE_MS
  const kept = previous.filter((point) => point.ts >= cutoff)
  const keys = new Set(kept.map((point) => point.key))
  for (const spot of spots) {
    const observedAt = now - Math.max(0, spot.ageSecs) * 1000
    const bucket = Math.floor(observedAt / 60_000)
    const key = `${spot.call}|${spot.band}|${spot.freqMhz.toFixed(4)}|${bucket}`
    if (keys.has(key)) continue
    keys.add(key)
    kept.push({ key, band: spot.band || '?', hour: new Date(observedAt).getUTCHours(), ts: observedAt })
  }
  return kept.sort((a, b) => a.ts - b.ts).slice(-HISTORY_MAX)
}

const TABS: { id: NeuralTab; icon: typeof Crosshair }[] = [
  { id: 'watcher', icon: MonitorUp },
  { id: 'hunt', icon: Crosshair },
  { id: 'insight', icon: BrainCircuit },
  { id: 'propagation', icon: Activity },
  { id: 'weather', icon: CloudLightning },
  { id: 'satellites', icon: Orbit },
  { id: 'briefing', icon: Newspaper },
]

// Radio measurement units are protocol/UI invariants, not localizable prose.
const FREQUENCY_UNIT = 'MHz'
const DISTANCE_UNIT = 'km'

function tabLabel(tab: NeuralTab): string {
  switch (tab) {
    case 'watcher': return t('neuralDx.tab.watcher')
    case 'hunt': return t('neuralDx.tab.hunt')
    case 'insight': return t('neuralDx.tab.insight')
    case 'propagation': return t('neuralDx.tab.propagation')
    case 'weather': return t('neuralDx.tab.weather')
    case 'satellites': return t('neuralDx.tab.satellites')
    case 'briefing': return t('neuralDx.tab.briefing')
  }
}

function toWatcherSpot(spot: SpotRow, need: NeedAlert | undefined, myGrid: string) {
  const me = gridToLatLon(myGrid)
  const dx = spot.grid ? gridToLatLon(spot.grid) : null
  const timestamp = Date.now() / 1000 - Math.max(0, spot.ageSecs)
  const distanceKm = me && dx ? haversineKm(me, dx) : 0
  return {
    timestamp,
    time: new Date(timestamp * 1000).toISOString().slice(11, 16),
    freq: (spot.freqMhz * 1000).toFixed(1),
    dx_call: spot.call,
    de_call: spot.spotter,
    band: spot.band,
    mode: spot.submode ?? spot.mode,
    country: spot.entity,
    lat: dx?.lat ?? 0,
    lon: dx?.lon ?? 0,
    score: need?.priority ? need.priority * 100 : spot.workedAgoSecs == null ? 100 : 0,
    is_wanted: Boolean(need),
    is_rare: need?.tags.some((tag) => tag === 'NewEntity' || tag === 'Dxped') ?? false,
    via_eme: /\bEME\b/i.test(spot.comment),
    color: '#22d3ee',
    type: ['6m', '4m', '2m', '70cm', '23cm'].includes(spot.band) ? 'VHF' : 'HF',
    distance_km: distanceKm,
    spot_id: `${spot.call}-${Math.floor(timestamp)}`,
    comment: spot.comment,
    nexus_worked_ago_secs: spot.workedAgoSecs ?? null,
  }
}

async function loadQsoSummary(): Promise<WatcherQsoSummary> {
  const confirmedCalls = new Set<string>()
  const confirmedDxcc = new Set<string>()
  const workedCalls = new Set<string>()
  const workedDxcc = new Set<string>()
  const dxccByBand = new Map<string, Set<string>>()
  let totalQso = 0
  let totalConfirmed = 0
  let offset = 0
  while (true) {
    const page = await askLog({ kind: 'page', query: DEFAULT_LOG_QUERY, offset, limit: 1000 })
    totalQso = page.total
    for (const row of page.rows as LoggedQso[]) {
      const call = row.call.trim().toUpperCase()
      const entity = row.entity ?? row.country ?? ''
      const confirmed = row.awardConfirmed || row.confirmed
      if (call) workedCalls.add(call)
      if (entity) workedDxcc.add(entity)
      if (confirmed) {
        totalConfirmed += 1
        if (call) confirmedCalls.add(call)
        if (entity) {
          confirmedDxcc.add(entity)
          if (!dxccByBand.has(row.band)) dxccByBand.set(row.band, new Set())
          dxccByBand.get(row.band)?.add(entity)
        }
      }
    }
    offset += page.rows.length
    if (offset >= page.total || page.rows.length === 0) {
      return {
        confirmedCalls: [...confirmedCalls], confirmedDxcc: [...confirmedDxcc],
        workedCalls: [...workedCalls], workedDxcc: [...workedDxcc],
        dxccByBand: Object.fromEntries([...dxccByBand].map(([band, values]) => [band, [...values]])),
        totalQso, totalConfirmed,
      }
    }
  }
}

export function NeuralDxView({ spots, needs, myCall, myGrid, logTick, selectedCall, onSelect, onWork, onNavigate }: Props) {
  const [tab, setTab] = useState<NeuralTab>('watcher')
  const [history, setHistory] = useState<HistoryPoint[]>(() => loadJson(HISTORY_KEY, []))
  const [watch, setWatch] = useState<string[]>(() => loadJson(WATCH_KEY, []))
  const [runtime, setRuntime] = useState<NeuralDxStatus | null>(null)
  const [runtimeError, setRuntimeError] = useState('')
  const [qsoSummary, setQsoSummary] = useState<WatcherQsoSummary>(EMPTY_QSO_SUMMARY)

  useEffect(() => {
    setHistory((current) => {
      const next = recordHistory(spots, current)
      saveJson(HISTORY_KEY, next)
      return next
    })
  }, [spots])

  useEffect(() => {
    let live = true
    const refresh = () => getNeuralDxStatus().then((next) => {
      if (live) setRuntime(next)
    }).catch((error) => {
      if (live) setRuntimeError(String(error))
    })
    void refresh()
    const timer = window.setInterval(refresh, runtime?.phase === 'ready' ? 5000 : 700)
    return () => { live = false; window.clearInterval(timer) }
  }, [runtime?.phase])

  useEffect(() => {
    if (runtime?.phase !== 'ready') return
    let live = true
    loadQsoSummary().then((summary) => {
      if (live) setQsoSummary(summary)
    }).catch((error) => { if (live) setRuntimeError(String(error)) })
    return () => { live = false }
  }, [runtime?.phase, logTick])

  useEffect(() => {
    if (runtime?.phase !== 'ready') return
    let live = true
    const needByCall = new Map(needs.map((need) => [`${need.call.toUpperCase()}|${need.band}`, need]))
    syncNeuralDx({
      station: { callsign: myCall, grid: myGrid },
      spots: spots.map((spot) => toWatcherSpot(spot, needByCall.get(`${spot.call.toUpperCase()}|${spot.band}`), myGrid)),
      qsoSummary,
    }).catch((error) => { if (live) setRuntimeError(String(error)) })
    return () => { live = false }
  }, [runtime?.phase, spots, needs, myCall, myGrid, qsoSummary])

  useEffect(() => {
    if (runtime?.phase !== 'ready') return
    let live = true
    const forwardActions = async () => {
      const actions = await getNeuralDxActions()
      for (const action of actions) {
        if (action.type === 'postSpot') await postSpot(action.freqMhz, action.call, action.comment)
      }
    }
    const tick = () => void forwardActions().catch((error) => { if (live) setRuntimeError(String(error)) })
    tick()
    const timer = window.setInterval(tick, 1000)
    return () => { live = false; window.clearInterval(timer) }
  }, [runtime?.phase])

  const targets = useMemo(() => scoreHuntTargets(spots, needs, myGrid), [spots, needs, myGrid])
  const bandCounts = useMemo(() => {
    const counts = new Map<string, number>()
    for (const spot of spots) counts.set(spot.band || '?', (counts.get(spot.band || '?') ?? 0) + 1)
    return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 10)
  }, [spots])
  const hourCounts = useMemo(() => {
    const counts = Array.from({ length: 24 }, () => 0)
    for (const point of history) counts[point.hour] += 1
    return counts
  }, [history])
  const maxHour = Math.max(1, ...hourCounts)
  const recent = history.filter((point) => point.ts >= Date.now() - 60 * 60 * 1000).length
  const previous = history.filter(
    (point) => point.ts >= Date.now() - 2 * 60 * 60 * 1000 && point.ts < Date.now() - 60 * 60 * 1000,
  ).length
  const trend = previous === 0 ? (recent > 0 ? 100 : 0) : Math.round(((recent - previous) / previous) * 100)

  const toggleWatch = (call: string) => {
    const key = call.toUpperCase()
    const next = watch.includes(key) ? watch.filter((item) => item !== key) : [...watch, key].slice(-100)
    setWatch(next)
    saveJson(WATCH_KEY, next)
  }

  const bridge = (view: View, title: string, body: string, button: string) => (
    <section className="neural-dx-bridge panel">
      <RadioTower size={30} aria-hidden="true" />
      <div>
        <h2>{title}</h2>
        <p>{body}</p>
      </div>
      <button type="button" className="btn primary" onClick={() => onNavigate(view)}>
        {button}
      </button>
    </section>
  )

  return (
    <main className="neural-dx-view">
      <header className="neural-dx-header">
        <div>
          <span className="neural-dx-kicker">{t('neuralDx.product')}</span>
          <h1>{t('features.neuralDx.label')}</h1>
          <p>{t('neuralDx.subtitle')}</p>
        </div>
        <div className="neural-dx-live" role="status">
          <span aria-hidden="true" />
          {t('neuralDx.live', { count: spots.length })}
        </div>
      </header>

      <nav className="neural-dx-tabs" aria-label={t('neuralDx.tabs.aria')}>
        {TABS.map(({ id, icon: Icon }) => (
          <button
            key={id}
            type="button"
            className={tab === id ? 'active' : ''}
            aria-current={tab === id ? 'page' : undefined}
            onClick={() => setTab(id)}
          >
            <Icon size={16} aria-hidden="true" />
            {tabLabel(id)}
          </button>
        ))}
      </nav>

      {tab === 'watcher' && (
        <section className="neural-dx-watcher panel">
          {runtime?.phase === 'ready' && runtime.url ? (
            <iframe title={t('neuralDx.product')} src={runtime.url} />
          ) : (
            <div role="status">
              <BrainCircuit size={34} aria-hidden="true" />
              <h2>{runtime?.phase === 'unavailable' ? t('neuralDx.runtime.unavailable') : t('neuralDx.runtime.starting')}</h2>
              <p>{runtime?.detail || runtimeError || t('neuralDx.runtime.wait')}</p>
              {runtime?.phase === 'unavailable' && (
                <button type="button" className="btn primary" onClick={() => void restartNeuralDx()}>
                  <RotateCcw size={15} aria-hidden="true" /> {t('neuralDx.runtime.restart')}
                </button>
              )}
            </div>
          )}
        </section>
      )}

      {tab === 'hunt' && (
        <div className="neural-dx-hunt">
          {targets[0] ? (
            <section className="neural-dx-hero panel">
              <span className="neural-dx-rank">#1</span>
              <div>
                <span className="neural-dx-entity">{targets[0].spot.entity || t('neuralDx.unknownEntity')}</span>
                <button type="button" className="neural-dx-call" onClick={() => onSelect(targets[0].spot.call)}>
                  {targets[0].spot.call}
                </button>
                <p>
                  {targets[0].spot.band} · {targets[0].spot.submode ?? targets[0].spot.mode} ·{' '}
                  {targets[0].spot.freqMhz.toFixed(4)} {FREQUENCY_UNIT}
                </p>
                <div className="neural-dx-badges">
                  {targets[0].need?.tags.map((tag) => <span key={tag}>{tag}</span>)}
                  {targets[0].spot.workedAgoSecs == null && <span>{t('neuralDx.badge.unworked')}</span>}
                  {targets[0].spot.corroborators.length > 0 && <span>{t('neuralDx.badge.corroborated')}</span>}
                </div>
              </div>
              <div className="neural-dx-hero-actions">
                <strong>{Math.round(targets[0].score)}</strong>
                <span>{t('neuralDx.score')}</span>
                <button type="button" className="btn primary" onClick={() => onWork(targets[0].spot)}>
                  {t('neuralDx.work')}
                </button>
                <button type="button" className="btn" onClick={() => toggleWatch(targets[0].spot.call)}>
                  {watch.includes(targets[0].spot.call.toUpperCase()) ? t('neuralDx.unwatch') : t('neuralDx.watch')}
                </button>
              </div>
            </section>
          ) : (
            <section className="panel neural-dx-empty" role="status">{t('neuralDx.empty')}</section>
          )}

          <section className="panel neural-dx-targets">
            <h2>{t('neuralDx.targets')}</h2>
            <div className="neural-dx-target-list">
              {targets.slice(1, 16).map((target, index) => (
                <button
                  key={`${target.spot.call}-${target.spot.band}`}
                  type="button"
                  className={selectedCall === target.spot.call ? 'selected' : ''}
                  onClick={() => onSelect(target.spot.call)}
                  onDoubleClick={() => onWork(target.spot)}
                >
                  <span>{index + 2}</span>
                  <strong>{target.spot.call}</strong>
                  <em>{target.spot.entity || 'DX'}</em>
                  <b>{target.spot.band}</b>
                  <small>{target.distanceKm == null ? '—' : `${Math.round(target.distanceKm)} ${DISTANCE_UNIT}`}</small>
                  <output>{Math.round(target.score)}</output>
                </button>
              ))}
            </div>
          </section>
        </div>
      )}

      {tab === 'insight' && (
        <div className="neural-dx-insight">
          <section className="panel neural-dx-metric">
            <span>{t('neuralDx.insight.recent')}</span><strong>{recent}</strong><small>{trend >= 0 ? '+' : ''}{trend}%</small>
          </section>
          <section className="panel neural-dx-metric">
            <span>{t('neuralDx.insight.maturity')}</span><strong>{history.length}</strong><small>{t('neuralDx.insight.samples')}</small>
          </section>
          <section className="panel neural-dx-heatmap">
            <h2>{t('neuralDx.insight.opens')}</h2>
            <div>
              {hourCounts.map((count, hour) => (
                <span key={hour} style={{ '--heat': String(count / maxHour) } as CSSProperties} title={`${hour}:00Z · ${count}`}>
                  {String(hour).padStart(2, '0')}
                </span>
              ))}
            </div>
          </section>
          <section className="panel neural-dx-bands">
            <h2>{t('neuralDx.insight.bands')}</h2>
            {bandCounts.map(([band, count]) => (
              <div key={band}><span>{band}</span><i style={{ '--share': String(count / Math.max(1, bandCounts[0]?.[1] ?? 1)) } as CSSProperties} /><strong>{count}</strong></div>
            ))}
          </section>
        </div>
      )}

      {tab === 'propagation' && bridge('connect', t('neuralDx.propagation.title'), t('neuralDx.propagation.body'), t('neuralDx.propagation.open'))}
      {tab === 'weather' && bridge('connect', t('neuralDx.weather.title'), t('neuralDx.weather.body'), t('neuralDx.weather.open'))}
      {tab === 'satellites' && bridge('sats', t('neuralDx.satellites.title'), t('neuralDx.satellites.body'), t('neuralDx.satellites.open'))}
      {tab === 'briefing' && bridge('dxped', t('neuralDx.briefing.title'), t('neuralDx.briefing.body'), t('neuralDx.briefing.open'))}
    </main>
  )
}
