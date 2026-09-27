import { useCallback, useEffect, useMemo, useState } from 'react'
import { Activity, Crosshair, Map, RadioTower, RefreshCw, Search, Settings2, ShieldCheck, Target } from 'lucide-react'
import {
  clearDxChaserTwilio, configureDxChaser, getDxChaser, getDxChaserDxccMatrix,
  getFeedHealth, resetDxChaserStatistics, setDxChaserArmed, setDxChaserTwilio, stopDxChaser,
  testDxChaserTwilio, type DxChaserDxccMatrix, type DxChaserSettings,
  type DxChaserSnapshot, type DxChaserStatistics,
} from '../api'
import type { FeedHealth, FeedStatus } from '../types'
import { t } from '../i18n'
import './DxChaserView.css'

type Tab = 'targets' | 'dxcc' | 'bands' | 'cache' | 'log' | 'statistics' | 'settings'
type DxccFilter = 'all' | 'unworked' | 'worked' | 'spottedUnworked'
type TranslationKey = Parameters<typeof t>[0]
const DB_UNIT = 'dB'
const HZ_UNIT = 'Hz'
const BANDS = ['160m', '80m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m']
type NumberKey = { [K in keyof DxChaserSettings]: DxChaserSettings[K] extends number ? K : never }[keyof DxChaserSettings]
type NumberField = { key: NumberKey; labelKey: TranslationKey; step: number; min: number; max: number }
const numberFields: NumberField[] = [
  { key: 'missedCycles', labelKey: 'dxChaser.field.missedCycles', step: 1, min: 1, max: 100 }, { key: 'scarceMissedCycles', labelKey: 'dxChaser.field.scarceMissedCycles', step: 1, min: 1, max: 100 }, { key: 'atnoHardMaxCycles', labelKey: 'dxChaser.field.atnoHardMaxCycles', step: 1, min: 1, max: 1000 }, { key: 'scarceThreshold', labelKey: 'dxChaser.field.scarceThreshold', step: 1, min: 0, max: 20 },
  { key: 'cooldownSecs', labelKey: 'dxChaser.field.cooldownSecs', step: 1, min: 1, max: 86400 }, { key: 'atnoCooldownSecs', labelKey: 'dxChaser.field.atnoCooldownSecs', step: 1, min: 1, max: 604800 }, { key: 'decodeTtlSecs', labelKey: 'dxChaser.field.decodeTtlSecs', step: 1, min: 1, max: 3600 }, { key: 'spotTtlSecs', labelKey: 'dxChaser.field.spotTtlSecs', step: 1, min: 1, max: 3600 }, { key: 'crossBandAlertCooldownSecs', labelKey: 'dxChaser.field.crossBandAlertCooldownSecs', step: 1, min: 1, max: 86400 }, { key: 'localVerificationSecs', labelKey: 'dxChaser.field.localVerificationSecs', step: 1, min: 1, max: 3600 }, { key: 'nonstandardQsyCooldownSecs', labelKey: 'dxChaser.field.nonstandardQsyCooldownSecs', step: 1, min: 1, max: 86400 }, { key: 'nonstandardThresholdHz', labelKey: 'dxChaser.field.nonstandardThresholdHz', step: 1, min: 1, max: 100000 }, { key: 'minExternalSpotters', labelKey: 'dxChaser.field.minExternalSpotters', step: 1, min: 1, max: 20 },
  { key: 'minDwellSecs', labelKey: 'dxChaser.field.minDwellSecs', step: 1, min: 30, max: 86400 }, { key: 'hysteresis', labelKey: 'dxChaser.field.hysteresis', step: 0.05, min: 0.1, max: 0.5 }, { key: 'quietSpotThreshold', labelKey: 'dxChaser.field.quietSpotThreshold', step: 1, min: 1, max: 100000 }, { key: 'pounceCooldownSecs', labelKey: 'dxChaser.field.pounceCooldownSecs', step: 1, min: 1, max: 86400 }, { key: 'deferredPounceExpireSecs', labelKey: 'dxChaser.field.deferredPounceExpireSecs', step: 1, min: 1, max: 86400 }, { key: 'maxDeferredPounces', labelKey: 'dxChaser.field.maxDeferredPounces', step: 1, min: 1, max: 10 }, { key: 'utcOffsetHours', labelKey: 'dxChaser.field.utcOffsetHours', step: 1, min: -12, max: 14 }, { key: 'decodeHistoryRetentionCycles', labelKey: 'dxChaser.field.decodeHistoryRetentionCycles', step: 1, min: 2, max: 1000 },
  { key: 'slotMinHz', labelKey: 'dxChaser.field.slotMinHz', step: 1, min: 0, max: 3000 }, { key: 'slotMaxHz', labelKey: 'dxChaser.field.slotMaxHz', step: 1, min: 0, max: 3000 }, { key: 'slotWidthHz', labelKey: 'dxChaser.field.slotWidthHz', step: 1, min: 1, max: 1000 }, { key: 'txOffsetDeltaHz', labelKey: 'dxChaser.field.txOffsetDeltaHz', step: 1, min: -3000, max: 3000 },
  { key: 'scoreNewDxcc', labelKey: 'dxChaser.field.scoreNewDxcc', step: 1, min: 0, max: 100000 }, { key: 'scoreNewBand', labelKey: 'dxChaser.field.scoreNewBand', step: 1, min: 0, max: 100000 }, { key: 'scoreNewGrid', labelKey: 'dxChaser.field.scoreNewGrid', step: 1, min: 0, max: 100000 }, { key: 'scoreCq', labelKey: 'dxChaser.field.scoreCq', step: 1, min: 0, max: 100000 }, { key: 'scoreSnrPerDb', labelKey: 'dxChaser.field.scoreSnrPerDb', step: 1, min: 0, max: 100000 }, { key: 'scoreRepeat', labelKey: 'dxChaser.field.scoreRepeat', step: 1, min: 0, max: 100000 }, { key: 'scoreSpotter', labelKey: 'dxChaser.field.scoreSpotter', step: 1, min: 0, max: 100000 }, { key: 'scoreCallingMe', labelKey: 'dxChaser.field.scoreCallingMe', step: 1, min: 0, max: 100000 }, { key: 'minSnr', labelKey: 'dxChaser.field.minSnr', step: 1, min: -30, max: 0 },
]

const generalSettings: Array<{ key: keyof DxChaserSettings; labelKey: TranslationKey }> = [
  { key: 'enabled', labelKey: 'dxChaser.field.enabled' }, { key: 'autoBand', labelKey: 'dxChaser.autoBand' }, { key: 'hunterMode', labelKey: 'dxChaser.field.hunterMode' }, { key: 'scarceMode', labelKey: 'dxChaser.field.scarceMode' }, { key: 'autoQsyNonstandard', labelKey: 'dxChaser.nonstandard' }, { key: 'ownCountryFilter', labelKey: 'dxChaser.ownCountry' }, { key: 'usGridOnly', labelKey: 'dxChaser.usGrid' }, { key: 'timeBasedExclusions', labelKey: 'dxChaser.field.timeBasedExclusions' },
]
const feedSettings: Array<{ key: keyof DxChaserSettings; labelKey: TranslationKey }> = [
  { key: 'useRbn', labelKey: 'dxChaser.feed.useRbn' }, { key: 'usePskReporter', labelKey: 'dxChaser.feed.usePskReporter' }, { key: 'useDxCluster', labelKey: 'dxChaser.feed.useDxCluster' }, { key: 'usSpottersOnly', labelKey: 'dxChaser.feed.usSpottersOnly' },
]
const twilioFields = [
  { key: 'accountSid', labelKey: 'dxChaser.twilio.accountSid' }, { key: 'authToken', labelKey: 'dxChaser.twilio.authToken' }, { key: 'fromNumber', labelKey: 'dxChaser.twilio.fromNumber' }, { key: 'toNumber', labelKey: 'dxChaser.twilio.toNumber' },
] as const

export function DxChaserView({ initialModel }: { initialModel?: DxChaserSnapshot } = {}) {
  const [model, setModel] = useState<DxChaserSnapshot | null>(initialModel ?? null)
  const [draft, setDraft] = useState<DxChaserSettings | null>(initialModel?.settings ?? null)
  const [tab, setTab] = useState<Tab>('targets')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [saving, setSaving] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [matrix, setMatrix] = useState<DxChaserDxccMatrix | null>(null)
  const [matrixLoading, setMatrixLoading] = useState(false)
  const [matrixError, setMatrixError] = useState('')
  const [dxccSearch, setDxccSearch] = useState('')
  const [dxccFilter, setDxccFilter] = useState<DxccFilter>('all')
  const [twilio, setTwilio] = useState({ accountSid: '', authToken: '', fromNumber: '', toNumber: '' })
  const [feedHealth, setFeedHealth] = useState<FeedHealth | null>(null)

  useEffect(() => {
    if (initialModel) return
    let live = true
    const load = () => getDxChaser().then(value => {
      if (live) {
        setModel(value)
        setDraft(current => current ?? value.settings)
        setError('')
      }
    }).catch(reason => { if (live) setError(String(reason)) })
    void load()
    const timer = window.setInterval(load, 3000)
    return () => { live = false; window.clearInterval(timer) }
  }, [initialModel])

  useEffect(() => {
    let live = true
    const load = () => getFeedHealth().then(value => { if (live) setFeedHealth(value) }).catch(() => {})
    void load()
    const timer = window.setInterval(load, 10_000)
    return () => { live = false; window.clearInterval(timer) }
  }, [])

  const loadMatrix = useCallback(async () => {
    setMatrixLoading(true)
    try {
      setMatrix(await getDxChaserDxccMatrix())
      setMatrixError('')
    } catch (reason) {
      setMatrixError(String(reason))
    } finally {
      setMatrixLoading(false)
    }
  }, [])

  useEffect(() => {
    if (tab !== 'dxcc') return
    void loadMatrix()
    const timer = window.setInterval(() => { void loadMatrix() }, 30_000)
    return () => window.clearInterval(timer)
  }, [loadMatrix, tab])

  if (error && !model) return <main className="dx-chaser-shell"><div className="dx-chaser-error">{t('dxChaser.error', { error })}</div></main>
  if (!model || !draft) return <main className="dx-chaser-shell"><div className="dx-chaser-loading">{t('dxChaser.loading')}</div></main>

  const setSetting = <K extends keyof DxChaserSettings>(key: K, value: DxChaserSettings[K]) => {
    setDraft({ ...draft, [key]: value })
    setDirty(true)
  }
  const run = async (work: () => Promise<DxChaserSnapshot>) => {
    setSaving(true)
    try { const next = await work(); setModel(next); setError('') }
    catch (reason) { setError(String(reason)) }
    finally { setSaving(false) }
  }
  const save = () => run(async () => {
    const next = await configureDxChaser(draft)
    setDraft(next.settings)
    setDirty(false)
    return next
  })
  const tabs: Array<[Tab, string]> = [
    ['targets', t('dxChaser.candidates')], ['dxcc', t('dxChaser.dxcc')],
    ['bands', t('dxChaser.bands')], ['cache', t('dxChaser.cache')],
    ['log', t('dxChaser.log')], ['statistics', t('dxChaser.statistics')],
    ['settings', t('dxChaser.settings')],
  ]

  return <main className="dx-chaser-shell">
    <header className="dx-chaser-head"><div><h1><Crosshair size={22} />{t('dxChaser.title')}</h1><p>{t('dxChaser.subtitle')}</p></div><div className="dx-chaser-actions"><button className={model.automationArmed ? 'danger' : 'primary'} disabled={!model.settings.enabled || saving} onClick={() => run(() => setDxChaserArmed(!model.automationArmed))}>{model.automationArmed ? t('dxChaser.disarm') : t('dxChaser.arm')}</button><button className="danger" onClick={() => run(stopDxChaser)}>{t('dxChaser.emergencyStop')}</button></div></header>
    <section className="dx-chaser-status"><div><Activity size={16} /><strong>{model.state}</strong></div><div><ShieldCheck size={16} /><span>{t('dxChaser.txGuard')}</span><strong>{model.txGuard}</strong></div><div><span>{t('dxChaser.automation')}</span><strong>{model.automationArmed ? t('dxChaser.armed') : t('dxChaser.disarmed')}</strong></div><div><span>{t('dxChaser.activeSpotPairs')}</span><strong>{model.activeSpotPairs}</strong></div>{feedHealth && <><FeedBadge label={t('dxChaser.feed.clusterAggregate')} status={feedHealth.cluster} /><FeedBadge label={t('dxChaser.feed.humanCluster')} status={feedHealth.phoneCluster} detail={feedHealth.phoneClusterHost ? `${feedHealth.phoneClusterHost} · ${feedHealth.phoneSpotsSeen} ${t('dxChaser.feed.phoneSpots')}` : undefined} /><FeedBadge label={t('dxChaser.feed.pskReporter')} status={feedHealth.pskr} /></>}{(model.sources ?? []).map(source => <div key={source.source}><span className={source.healthy ? 'source-ok' : 'source-stale'}>●</span><span>{source.source}</span><strong>{source.items}</strong></div>)}<div><span>{t('dxChaser.upstream')}</span><code>{model.upstreamRevision.slice(0, 12)}</code></div></section>
    <p className="dx-chaser-guard">{t('dxChaser.masterHelp')}</p>
    {(error || model.lastError) && <div className="dx-chaser-error compact">{error || model.lastError}</div>}{notice && <div className="dx-chaser-notice">{notice}</div>}
    {model.target && <section className="dx-chaser-engaged"><Target size={18} /><div><span>{t('dxChaser.target')}</span><strong>{model.target.candidate.call} · {model.target.candidate.country}</strong></div><b>{model.target.candidate.band}</b><b>{model.target.candidate.audioHz} {HZ_UNIT}</b><b>{model.target.candidate.snr} {DB_UNIT}</b><b>{t('dxChaser.cyclesRemaining')}: {model.targetCyclesRemaining ?? 0}</b></section>}
    <nav className="dx-chaser-tabs" role="tablist" aria-label={t('dxChaser.views')}>{tabs.map(([id, label]) => <button key={id} id={`dx-chaser-tab-${id}`} role="tab" aria-selected={tab === id} aria-controls={`dx-chaser-panel-${id}`} className={tab === id ? 'active' : ''} onClick={() => setTab(id)}>{label}</button>)}</nav>
    <div className="dx-chaser-scroll" id={`dx-chaser-panel-${tab}`} role="tabpanel" aria-labelledby={`dx-chaser-tab-${tab}`}>
      {tab === 'targets' && (model.candidates.length ? <div className="dx-chaser-grid">{model.candidates.map(candidate => <article key={`${candidate.call}-${candidate.band}`} className="dx-chaser-card"><div><strong>{candidate.call}</strong><span>{candidate.country}</span></div><div className="dx-chaser-badges"><b>{candidate.band}</b><span>{candidate.snr} {DB_UNIT}</span>{candidate.newDxcc && <em>{t('dxChaser.need.atno')}</em>}{candidate.newBand && <em>{t('dxChaser.need.newBand')}</em>}{candidate.newGrid && <em>{t('dxChaser.need.newGrid')}</em>}</div><p>{candidate.message}</p><dl><div><dt>{t('dxChaser.score')}</dt><dd>{candidate.score}</dd></div><div><dt>{t('dxChaser.decodes')}</dt><dd>{candidate.decodeCount}</dd></div><div><dt>{t('dxChaser.spotters')}</dt><dd>{candidate.spotterCount}</dd></div><div><dt>{HZ_UNIT}</dt><dd>{candidate.audioHz}</dd></div></dl></article>)}</div> : <div className="dx-chaser-empty">{t('dxChaser.empty')}</div>)}
      {tab === 'dxcc' && <DxccBrowser matrix={matrix} loading={matrixLoading} error={matrixError} search={dxccSearch} filter={dxccFilter} onSearch={setDxccSearch} onFilter={setDxccFilter} onRefresh={loadMatrix} />}
      {tab === 'cache' && ((model.decodeHistory ?? []).length ? <div className="dx-chaser-list">{(model.decodeHistory ?? []).map(decode => <div key={decode.call}><time>{new Date(decode.lastHeardAt * 1000).toLocaleTimeString()}</time><b>{decode.call}</b><span>{decode.grid ?? '—'} · {decode.snr} {DB_UNIT} · {decode.audioHz} {HZ_UNIT} · ×{decode.timesHeard}</span></div>)}</div> : <div className="dx-chaser-empty">{t('dxChaser.emptyCache')}</div>)}
      {tab === 'bands' && <><div className="dx-chaser-grid">{model.bands.map(band => <article key={band.band} className="dx-chaser-card band"><RadioTower /><strong>{band.band}</strong><dl><div><dt>{t('dxChaser.decodes')}</dt><dd>{band.decodes}</dd></div><div><dt>{t('dxChaser.spotters')}</dt><dd>{band.spots}</dd></div><div><dt>{t('dxChaser.score')}</dt><dd>{band.score}</dd></div></dl></article>)}</div><h3>{t('dxChaser.bandHistory')}</h3><div className="dx-chaser-list">{(model.bandHistory ?? []).map((history, index) => <div key={`${history.endedAt}-${index}`}><time>{new Date(history.endedAt * 1000).toLocaleTimeString()}</time><b>{history.band}</b><span>{history.decodeCount} / {history.uniqueCalls} · {history.averageSnr.toFixed(1)} {DB_UNIT}</span></div>)}</div></>}
      {tab === 'log' && <div className="dx-chaser-list">{model.log.map((entry, index) => <div key={`${entry.at}-${index}`}><time>{new Date(entry.at * 1000).toLocaleTimeString()}</time><b>{entry.call ?? entry.kind}</b><span>{entry.detail}</span></div>)}</div>}
      {tab === 'statistics' && <section><h2>{t('dxChaser.sessionStatistics')}</h2><StatisticsGrid statistics={model.sessionStatistics} includeRate /><button onClick={() => run(resetDxChaserStatistics)}>{t('dxChaser.resetSession')}</button><h2>{t('dxChaser.allTimeStatistics')}</h2><StatisticsGrid statistics={model.allTimeStatistics} /></section>}
      {tab === 'settings' && <section className="dx-chaser-settings"><h2><Settings2 size={18} />{t('dxChaser.settings')}</h2><div className="dx-chaser-setting-group"><h3>{t('dxChaser.general')}</h3>{generalSettings.map(({ key, labelKey }) => <label key={key}><span>{t(labelKey)}</span><input type="checkbox" checked={Boolean(draft[key])} onChange={event => setSetting(key, event.target.checked as never)} /></label>)}</div><div className="dx-chaser-setting-group"><h3>{t('dxChaser.feed.sources')}</h3>{feedSettings.map(({ key, labelKey }) => <label key={key}><span>{t(labelKey)}</span><input type="checkbox" checked={Boolean(draft[key])} onChange={event => setSetting(key, event.target.checked as never)} /></label>)}<p>{t('dxChaser.feed.sharedAuthority')}</p></div>
        <div className="dx-chaser-setting-group"><h3>{t('dxChaser.enabledBands')}</h3><div className="dx-chaser-band-picker">{BANDS.map(band => <label key={band}><span>{band}</span><input type="checkbox" checked={draft.enabledBands.includes(band)} onChange={event => setSetting('enabledBands', event.target.checked ? [...draft.enabledBands, band] : draft.enabledBands.filter(item => item !== band))} /></label>)}</div></div>
        <div className="dx-chaser-setting-group"><h3>{t('dxChaser.advanced')}</h3><div className="dx-chaser-number-grid">{numberFields.map(({ key, labelKey, step, min, max }) => <label key={key}><span>{t(labelKey)}</span><input type="number" step={step} min={min} max={max} value={draft[key]} onChange={event => setSetting(key, Number(event.target.value) as never)} /></label>)}<label><span>{t('dxChaser.field.atnoChasePolicy')}</span><select value={draft.atnoChasePolicy} onChange={event => setSetting('atnoChasePolicy', event.target.value as DxChaserSettings['atnoChasePolicy'])}><option value="disabled">{t('dxChaser.atno.disabled')}</option><option value="allTime">{t('dxChaser.atno.allTime')}</option><option value="bandSlot">{t('dxChaser.atno.bandSlot')}</option><option value="both">{t('dxChaser.atno.both')}</option></select></label><label><span>{t('dxChaser.field.quietHoursStart')}</span><input type="number" min="0" max="23" value={draft.quietHoursStart ?? ''} onChange={event => setSetting('quietHoursStart', event.target.value === '' ? null : Number(event.target.value))} /></label><label><span>{t('dxChaser.field.quietHoursEnd')}</span><input type="number" min="0" max="23" value={draft.quietHoursEnd ?? ''} onChange={event => setSetting('quietHoursEnd', event.target.value === '' ? null : Number(event.target.value))} /></label></div></div>
        <div className="dx-chaser-setting-group"><h3>{t('dxChaser.sms')}</h3><label><span>{t('dxChaser.smsMode')}</span><select value={draft.smsMode} onChange={event => setSetting('smsMode', event.target.value as DxChaserSettings['smsMode'])}><option value="disabled">{t('dxChaser.smsDisabled')}</option><option value="everyTx">{t('dxChaser.smsEveryTx')}</option><option value="qsoCompleteOnly">{t('dxChaser.smsComplete')}</option></select></label><label><span>{t('dxChaser.field.smsMinIntervalSecs')}</span><input type="number" min="10" max="3600" step="1" value={draft.smsMinIntervalSecs} onChange={event => setSetting('smsMinIntervalSecs', Number(event.target.value))} /></label>{twilioFields.map(({ key, labelKey }) => <label key={key}><span>{t(labelKey)}</span><input type={key === 'authToken' ? 'password' : 'text'} autoComplete="off" value={twilio[key]} onChange={event => setTwilio({ ...twilio, [key]: event.target.value })} /></label>)}<div className="dx-chaser-actions"><button onClick={async () => { try { await setDxChaserTwilio(twilio); setNotice(t('dxChaser.twilioSaved')) } catch (reason) { setError(String(reason)) } }}>{t('dxChaser.twilioSave')}</button><button onClick={async () => { try { await testDxChaserTwilio(); setNotice(t('dxChaser.twilioTested')) } catch (reason) { setError(String(reason)) } }}>{t('dxChaser.twilioTest')}</button><button onClick={async () => { try { await clearDxChaserTwilio(); setTwilio({ accountSid: '', authToken: '', fromNumber: '', toNumber: '' }); setNotice(t('dxChaser.twilioCleared')) } catch (reason) { setError(String(reason)) } }}>{t('dxChaser.twilioClear')}</button></div></div>
        <div className="dx-chaser-actions sticky"><button className="primary" disabled={saving || !dirty} onClick={save}>{t('dxChaser.save')}</button><button disabled={!dirty} onClick={() => { setDraft(model.settings); setDirty(false) }}>{t('dxChaser.cancel')}</button>{dirty && <span>{t('dxChaser.unsaved')}</span>}</div></section>}
    </div>
  </main>
}

function FeedBadge({ label, status, detail }: { label: string; status: FeedStatus; detail?: string }) {
  const state = status.enabled ? status.state : t('dxChaser.feed.off')
  const age = status.lastEventSecs == null ? '' : ` · ${t('dxChaser.feed.age', { seconds: status.lastEventSecs })}`
  return <div title={detail}><span className={status.state === 'live' || status.state === 'connected' ? 'source-ok' : 'source-stale'}>●</span><span>{label}</span><strong>{state}{age}</strong>{detail && <small>{detail}</small>}</div>
}

function StatisticsGrid({ statistics, includeRate = false }: { statistics: DxChaserStatistics; includeRate?: boolean }) {
  const values: Array<[string, number]> = [
    [t('dxChaser.evaluated'), statistics.evaluated],
    [t('dxChaser.selected'), statistics.selected],
    [t('dxChaser.worked'), statistics.completed],
    [t('dxChaser.successPercent'), statistics.selected ? Math.round(statistics.completed / statistics.selected * 100) : 0],
    [t('dxChaser.newDxccWorked'), statistics.newDxccWorked],
    [t('dxChaser.newBandSlots'), statistics.newBandSlots],
    [t('dxChaser.newGrids'), statistics.newGrids],
    [t('dxChaser.abandoned'), statistics.abandoned],
    [t('dxChaser.bandChanges'), statistics.bandChanges],
    [t('dxChaser.nonstandardQsys'), statistics.nonstandardQsys],
    [t('dxChaser.smsQueued'), statistics.smsQueued],
  ]
  if (includeRate) values.splice(4, 0, [t('dxChaser.qsoPerHour'), statistics.startedAt ? Number((statistics.completed / Math.max(1, (Date.now() / 1000 - statistics.startedAt) / 3600)).toFixed(1)) : 0])
  return <div className="dx-chaser-stats">{values.map(([label, value]) => <article key={label}><strong>{value}</strong><span>{label}</span></article>)}</div>
}

function DxccBrowser({ matrix, loading, error, search, filter, onSearch, onFilter, onRefresh }: {
  matrix: DxChaserDxccMatrix | null
  loading: boolean
  error: string
  search: string
  filter: DxccFilter
  onSearch: (value: string) => void
  onFilter: (value: DxccFilter) => void
  onRefresh: () => Promise<void>
}) {
  const entities = useMemo(() => {
    if (!matrix) return []
    const needle = search.trim().toLowerCase()
    return matrix.entities.filter(entity => {
      if (needle && !entity.searchText.includes(needle)) return false
      if (filter === 'unworked') return !entity.workedAny
      if (filter === 'worked') return entity.workedAny
      if (filter === 'spottedUnworked') return entity.hasActiveSpot
      return true
    })
  }, [filter, matrix, search])
  const filters: Array<{ id: DxccFilter; labelKey: TranslationKey; count: number }> = matrix ? [
    { id: 'all', labelKey: 'dxChaser.dxcc.all', count: matrix.filters.totalEntities },
    { id: 'unworked', labelKey: 'dxChaser.dxcc.unworked', count: matrix.filters.unworkedEntities },
    { id: 'worked', labelKey: 'dxChaser.dxcc.worked', count: matrix.filters.workedEntities },
    { id: 'spottedUnworked', labelKey: 'dxChaser.dxcc.spottedUnworked', count: matrix.filters.spottedUnworkedEntities },
  ] : []

  return <section className="dx-chaser-dxcc" aria-labelledby="dx-chaser-dxcc-heading">
    <div className="dx-chaser-dxcc-heading"><div><h2 id="dx-chaser-dxcc-heading"><Map size={18} />{t('dxChaser.dxcc.title')}</h2><p>{t('dxChaser.dxcc.help')}</p></div><button onClick={() => void onRefresh()} disabled={loading}><RefreshCw size={15} className={loading ? 'spinning' : ''} />{t('dxChaser.dxcc.refresh')}</button></div>
    <div className="dx-chaser-dxcc-tools">
      <label className="dx-chaser-dxcc-search"><span>{t('dxChaser.dxcc.search')}</span><div><Search size={15} /><input type="search" value={search} onChange={event => onSearch(event.target.value)} placeholder={t('dxChaser.dxcc.searchPlaceholder')} /></div></label>
      <div className="dx-chaser-dxcc-filters" role="group" aria-label={t('dxChaser.dxcc.filters')}>{filters.map(({ id, labelKey, count }) => <button key={id} className={filter === id ? 'active' : ''} aria-pressed={filter === id} onClick={() => onFilter(id)}>{t(labelKey)} <span>{count}</span></button>)}</div>
    </div>
    {error && <div className="dx-chaser-error compact" role="alert">{t('dxChaser.dxcc.error', { error })}</div>}
    {!matrix && loading && <div className="dx-chaser-loading" role="status">{t('dxChaser.dxcc.loading')}</div>}
    {matrix && <>
      <div className="dx-chaser-dxcc-summary" role="status">{t('dxChaser.dxcc.showing', { shown: entities.length, total: matrix.filters.totalEntities })}<span>{t('dxChaser.dxcc.generated', { time: new Date(matrix.generatedAt * 1000).toLocaleTimeString() })}</span></div>
      {entities.length ? <div className="dx-chaser-dxcc-table-wrap"><table className="dx-chaser-dxcc-table"><caption className="dx-chaser-visually-hidden">{t('dxChaser.dxcc.caption')}</caption><thead><tr><th scope="col">{t('dxChaser.dxcc.entity')}</th><th scope="col">{t('dxChaser.dxcc.prefix')}</th><th scope="col">{t('dxChaser.dxcc.rank')}</th>{matrix.bands.map(band => <th scope="col" key={band}>{band}</th>)}<th scope="col">{t('dxChaser.dxcc.activeSpots')}</th></tr></thead><tbody>{entities.map(entity => <tr key={entity.name}><th scope="row"><strong>{entity.name}</strong>{entity.workedAll && <span className="dx-chaser-dxcc-complete">{t('dxChaser.dxcc.complete')}</span>}</th><td><code>{entity.prefix || '—'}</code></td><td>{entity.rank ?? '—'}</td>{matrix.bands.map((band, index) => <td key={band} className={entity.worked[index] ? 'worked' : 'needed'} title={entity.worked[index] ? t('dxChaser.dxcc.slotWorked') : t('dxChaser.dxcc.slotNeeded')}><span aria-hidden="true">{entity.worked[index] ? '✓' : '·'}</span><span className="dx-chaser-visually-hidden">{entity.worked[index] ? t('dxChaser.dxcc.slotWorked') : t('dxChaser.dxcc.slotNeeded')}</span></td>)}<td>{entity.activeSpots.length ? <ul className="dx-chaser-dxcc-spots">{entity.activeSpots.map(spot => <li key={`${spot.call}-${spot.band}-${spot.freqMhz}-${spot.spotter}`}><strong>{spot.call}</strong><span>{spot.band} · {t('dxChaser.dxcc.frequencyMhz', { frequency: spot.freqMhz.toFixed(3) })}{spot.mode ? ` · ${spot.mode}` : ''}</span><small>{spot.source} · {spot.spotter} · {t('dxChaser.dxcc.ageSeconds', { count: spot.ageSeconds })}</small></li>)}</ul> : <span className="dx-chaser-dxcc-none">—</span>}</td></tr>)}</tbody></table></div> : <div className="dx-chaser-empty">{t('dxChaser.dxcc.empty')}</div>}
    </>}
  </section>
}
