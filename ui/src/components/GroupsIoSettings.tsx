import { useEffect, useState } from 'react'
import {
  clearGroupsIoCache,
  connectGroupsIo,
  disconnectGroupsIo,
  getGroupsIoStatus,
  openExternalUrl,
  saveGroupsIoSettings,
  syncGroupsIo,
  type GroupsIoGroupPolicy,
  type GroupsIoSettings as Settings,
  type GroupsIoStatus,
} from '../api'
import { confirmDialog } from '../confirm'
import { t } from '../i18n'
import { pushToast } from '../toast'
import './groups-io.css'

interface Props { onOpenGroupsIo: () => void }

const size = (value: number) => value < 1024 * 1024 ? `${Math.round(value / 1024)} KiB` : `${(value / 1024 / 1024).toFixed(1)} MiB`
const inheritedPolicy = (): GroupsIoGroupPolicy => ({ enabled: true, inheritGlobal: true, lookbackDays: 30, maximumMessages: 500, attachments: false, retentionDays: 180 })

export function GroupsIoSettings({ onOpenGroupsIo }: Props) {
  const [status, setStatus] = useState<GroupsIoStatus | null>(null)
  const [settings, setSettings] = useState<Settings | null>(null)
  const [apiKey, setApiKey] = useState('')
  const [busy, setBusy] = useState('')
  const [selectedGroups, setSelectedGroups] = useState<string[]>([])

  const apply = (next: GroupsIoStatus) => { setStatus(next); setSettings(next.settings) }
  useEffect(() => { void getGroupsIoStatus().then(apply).catch((error) => pushToast(String(error), 'error')) }, [])

  const run = async (label: string, action: () => Promise<GroupsIoStatus>) => {
    setBusy(label)
    try { const next = await action(); apply(next); pushToast(t('groupsio.action.complete', { label }), 'success'); return true }
    catch (error) { pushToast(typeof error === 'string' ? error : error instanceof Error ? error.message : t('groupsio.action.failed', { label }), 'error'); return false }
    finally { setBusy('') }
  }

  if (!settings || !status) return <p className="settings-note">{t('groupsio.settings.loading')}</p>
  const patch = <K extends keyof Settings>(key: K, value: Settings[K]) => setSettings((current) => current ? { ...current, [key]: value } : current)
  const updateGroup = (id: string, values: Partial<GroupsIoGroupPolicy>) => patch('groupOverrides', {
    ...settings.groupOverrides,
    [id]: { ...inheritedPolicy(), ...settings.groupOverrides[id], ...values },
  })
  const resetGroup = (id: string) => {
    const next = { ...settings.groupOverrides }
    delete next[id]
    patch('groupOverrides', next)
  }

  const clear = async (deleteAll: boolean) => {
    const ok = await confirmDialog({
      title: deleteAll ? t('groupsio.settings.deleteConfirmTitle') : t('groupsio.settings.clearConfirmTitle'),
      body: deleteAll
        ? t('groupsio.settings.deleteConfirmBody')
        : t('groupsio.settings.clearConfirmBody'),
      confirmLabel: deleteAll ? t('groupsio.settings.deleteLocal') : t('groupsio.settings.clearCache'),
      danger: true,
    })
    if (ok) await run(deleteAll ? t('groupsio.settings.deleteAction') : t('groupsio.settings.clearAction'), () => clearGroupsIoCache(deleteAll))
  }

  const applySelected = () => {
    const next = { ...settings.groupOverrides }
    for (const id of selectedGroups) next[id] = {
      enabled: true,
      inheritGlobal: false,
      lookbackDays: settings.defaultLookbackDays,
      maximumMessages: settings.defaultMaximumMessages,
      attachments: settings.attachmentAutoDownload,
      retentionDays: settings.offlineRetentionDays,
    }
    patch('groupOverrides', next)
  }
  const resetSelected = () => {
    const next = { ...settings.groupOverrides }
    for (const id of selectedGroups) delete next[id]
    patch('groupOverrides', next)
  }

  return (
    <div className="groupsio-settings">
      <p className="settings-note">{t('groupsio.settings.keyStorage')}</p>
      <div className="settings-grid groupsio-settings-grid">
        <div className="settings-field groupsio-account">
          <label className="settings-label" htmlFor="groupsio-api-key">{t('groupsio.settings.account')}</label>
          <span className="settings-hint">{t('groupsio.settings.accountHint')}</span>
          <div className="settings-input-row">
            <input id="groupsio-api-key" className="settings-input groupsio-api-key" type="password" autoComplete="new-password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} disabled={status.connected || Boolean(busy)} placeholder={status.connected ? t('groupsio.settings.keyStored') : t('groupsio.settings.keyPlaceholder')} />
            <button type="button" className="settings-linkbtn" onClick={() => void openExternalUrl('https://groups.io/settings/apikeys')}>{t('groupsio.settings.keyManage')}</button>
            <button type="button" className="primary" disabled={status.connected || !apiKey.trim() || Boolean(busy)} onClick={() => void run(t('groupsio.settings.connectVerify'), () => connectGroupsIo(apiKey)).then((ok) => { if (ok) setApiKey('') })}>{t('groupsio.settings.connectVerify')}</button>
          </div>
          <span className={`settings-hint${status.connected ? ' settings-ok' : ''}`}>{status.connected ? t('groupsio.settings.connectedCount', { count: status.counts.groups }) : t('groupsio.settings.notConnected')}</span>
        </div>

        <div className="settings-field"><label className="settings-toggle"><span className="settings-label">{t('groupsio.settings.enabled')}</span><button type="button" role="switch" aria-checked={settings.enabled} className={`toggle${settings.enabled ? ' on' : ''}`} onClick={() => patch('enabled', !settings.enabled)}><span className="toggle-knob" /></button></label><span className="settings-hint">{t('groupsio.settings.foregroundHint')}</span></div>
        <NumberSetting label={t('groupsio.settings.lookback')} value={settings.defaultLookbackDays} min={1} max={3650} onChange={(value) => patch('defaultLookbackDays', value)} />
        <NumberSetting label={t('groupsio.settings.maxMessages')} value={settings.defaultMaximumMessages} min={50} max={20000} onChange={(value) => patch('defaultMaximumMessages', value)} />
        <NumberSetting label={t('groupsio.settings.archiveBound')} value={settings.defaultArchiveDepth} min={100} max={100000} onChange={(value) => patch('defaultArchiveDepth', value)} />
        <NumberSetting label={t('groupsio.settings.refreshMinutes')} value={settings.foregroundRefreshMinutes} min={5} max={1440} onChange={(value) => patch('foregroundRefreshMinutes', value)} />
        <NumberSetting label={t('groupsio.settings.retention')} value={settings.offlineRetentionDays} min={7} max={3650} onChange={(value) => patch('offlineRetentionDays', value)} />
        <NumberSetting label={t('groupsio.settings.attachmentCeiling')} value={Math.round(settings.maximumAttachmentBytes / 1024 / 1024)} min={1} max={100} onChange={(value) => patch('maximumAttachmentBytes', value * 1024 * 1024)} />
        <NumberSetting label={t('groupsio.settings.draftRetention')} value={settings.draftRetentionDays} min={7} max={3650} onChange={(value) => patch('draftRetentionDays', value)} />
        <ToggleSetting label={t('groupsio.settings.refreshOnOpen')} value={settings.refreshOnOpen} onChange={(value) => patch('refreshOnOpen', value)} />
        <ToggleSetting label={t('groupsio.settings.autoAttachments')} value={settings.attachmentAutoDownload} onChange={(value) => patch('attachmentAutoDownload', value)} />
        <ToggleSetting label={t('groupsio.settings.includeMuted')} value={settings.includeMutedGroups} onChange={(value) => patch('includeMutedGroups', value)} />
        <ToggleSetting label={t('groupsio.settings.preview')} value={settings.showNewMessagePreview} onChange={(value) => patch('showNewMessagePreview', value)} />
      </div>

      <section className="groupsio-settings-overrides" aria-labelledby="groupsio-overrides-heading">
        <div>
          <h3 id="groupsio-overrides-heading">{t('groupsio.settings.overrides')}</h3>
          <p className="settings-note">{t('groupsio.settings.overridesHint')}</p>
          <div className="settings-actions"><button type="button" disabled={!selectedGroups.length} onClick={applySelected}>{t('groupsio.settings.applySelected')}</button><button type="button" disabled={!selectedGroups.length} onClick={resetSelected}>{t('groupsio.settings.resetSelected')}</button></div>
        </div>
        {status.groups.map((item) => {
          const explicit = settings.groupOverrides[item.id]
          const policy = { ...inheritedPolicy(), ...explicit }
          return <article key={item.id}>
            <div><label><input type="checkbox" aria-label={t('groupsio.settings.selectGroup', { name: item.title || item.name })} checked={selectedGroups.includes(item.id)} onChange={(event) => setSelectedGroups((current) => event.target.checked ? [...new Set([...current, item.id])] : current.filter((id) => id !== item.id))} /><strong>{item.title || item.name}</strong></label><small>{t('groupsio.settings.lastSync', { name: item.name, time: item.lastSyncUnix ? new Date(item.lastSyncUnix * 1000).toLocaleString() : t('groupsio.settings.never') })}</small></div>
            <label><span>{t('groupsio.settings.groupEnabled')}</span><input type="checkbox" checked={policy.enabled} onChange={(event) => updateGroup(item.id, { enabled: event.target.checked })} /></label>
            <label><span>{t('groupsio.settings.inherit')}</span><input type="checkbox" checked={policy.inheritGlobal} onChange={(event) => updateGroup(item.id, { inheritGlobal: event.target.checked })} /></label>
            {!policy.inheritGlobal && <>
              <label><span>{t('groupsio.settings.lookbackDays')}</span><input type="number" min={1} max={3650} value={policy.lookbackDays} onChange={(event) => updateGroup(item.id, { lookbackDays: Number(event.target.value) })} /></label>
              <label><span>{t('groupsio.settings.maximumMessages')}</span><input type="number" min={50} max={20000} value={policy.maximumMessages} onChange={(event) => updateGroup(item.id, { maximumMessages: Number(event.target.value) })} /></label>
              <label><span>{t('groupsio.settings.attachments')}</span><input type="checkbox" checked={policy.attachments} onChange={(event) => updateGroup(item.id, { attachments: event.target.checked })} /></label>
              <label><span>{t('groupsio.settings.retentionDays')}</span><input type="number" min={7} max={3650} value={policy.retentionDays} onChange={(event) => updateGroup(item.id, { retentionDays: Number(event.target.value) })} /></label>
            </>}
            <button type="button" disabled={!explicit} onClick={() => resetGroup(item.id)}>{t('groupsio.settings.resetOverride')}</button>
          </article>
        })}
        {!status.groups.length && <p className="settings-note">{t('groupsio.settings.noGroups')}</p>}
      </section>

      <div className="settings-actions">
        <button type="button" className="primary" disabled={Boolean(busy)} onClick={() => void run(t('groupsio.settings.saveAction'), () => saveGroupsIoSettings(settings))}>{t('groupsio.settings.save')}</button>
        <button type="button" disabled={!status.connected || Boolean(busy)} onClick={() => void run(t('groupsio.action.sync'), () => syncGroupsIo())}>{t('groupsio.settings.syncNow')}</button>
        <button type="button" onClick={onOpenGroupsIo}>{t('groupsio.settings.openScreen')}</button>
        <button type="button" disabled={!status.connected || Boolean(busy)} onClick={() => void run(t('groupsio.settings.disconnectAction'), disconnectGroupsIo)}>{t('groupsio.settings.disconnect')}</button>
      </div>
      <p className="settings-note">{t('groupsio.settings.status', { size: size(status.storageBytes), revision: status.apiRevision, policy: status.deliveryPolicy })}</p>

      <div className="settings-danger-zone">
        <button type="button" onClick={() => void clear(false)}>{t('groupsio.settings.clearCacheLong')}</button>
        <button type="button" className="danger" onClick={() => void clear(true)}>{t('groupsio.settings.deleteAll')}</button>
      </div>
    </div>
  )
}

function NumberSetting({ label, value, min, max, onChange }: { label: string; value: number; min: number; max: number; onChange(value: number): void }) {
  return <div className="settings-field"><label className="settings-inline-label"><span className="settings-label">{label}</span><input className="settings-input" type="number" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.target.value))} /></label></div>
}

function ToggleSetting({ label, value, onChange }: { label: string; value: boolean; onChange(value: boolean): void }) {
  return <div className="settings-field"><label className="settings-toggle"><span className="settings-label">{label}</span><button type="button" role="switch" aria-checked={value} className={`toggle${value ? ' on' : ''}`} onClick={() => onChange(!value)}><span className="toggle-knob" /></button></label></div>
}
