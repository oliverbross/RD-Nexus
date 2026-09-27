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

  const apply = (next: GroupsIoStatus) => { setStatus(next); setSettings(next.settings) }
  useEffect(() => { void getGroupsIoStatus().then(apply).catch((error) => pushToast(String(error), 'error')) }, [])

  const run = async (label: string, action: () => Promise<GroupsIoStatus>) => {
    setBusy(label)
    try { const next = await action(); apply(next); pushToast(`${label} complete`, 'success'); return true }
    catch (error) { pushToast(typeof error === 'string' ? error : error instanceof Error ? error.message : `${label} failed`, 'error'); return false }
    finally { setBusy('') }
  }

  if (!settings || !status) return <p className="settings-note">Loading Groups.io configuration…</p>
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
      title: deleteAll ? 'Delete all local Groups.io data?' : 'Clear downloaded Groups.io cache?',
      body: deleteAll
        ? 'This removes downloaded messages, local drafts and submission receipts. The API key remains in the system credential vault until you disconnect.'
        : 'Downloaded groups, topics and messages will be removed. Local drafts, submission receipts and the API key are preserved.',
      confirmLabel: deleteAll ? 'Delete local data' : 'Clear cache',
      danger: true,
    })
    if (ok) await run(deleteAll ? 'Local data deletion' : 'Cache clear', () => clearGroupsIoCache(deleteAll))
  }

  return (
    <>
      <p className="settings-note">The API key is stored by macOS Keychain, Windows Credential Manager, or Linux Secret Service. It is never written to settings.json, groupsio.json, logs, backups, or the UI after Connect.</p>
      <div className="settings-grid">
        <div className="settings-field settings-span-2">
          <span className="settings-label">Account</span>
          <span className="settings-hint">Create a Groups.io API key rather than entering your account password.</span>
          <div className="settings-input-row">
            <input className="settings-input" type="password" autoComplete="new-password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} disabled={status.connected || Boolean(busy)} placeholder={status.connected ? 'Stored in system credential vault' : 'Paste Groups.io API key'} />
            <button type="button" className="settings-linkbtn" onClick={() => void openExternalUrl('https://groups.io/settings/apikeys')}>Create / revoke key</button>
            <button type="button" className="primary" disabled={status.connected || !apiKey.trim() || Boolean(busy)} onClick={() => void run('Connect and verify', () => connectGroupsIo(apiKey)).then((ok) => { if (ok) setApiKey('') })}>Connect and verify</button>
          </div>
          <span className={`settings-hint${status.connected ? ' settings-ok' : ''}`}>{status.connected ? `Connected · ${status.counts.groups} memberships cached` : 'Not connected · downloaded content remains available'}</span>
        </div>

        <div className="settings-field"><label className="settings-toggle"><span className="settings-label">Foreground sync enabled</span><button type="button" role="switch" aria-checked={settings.enabled} className={`toggle${settings.enabled ? ' on' : ''}`} onClick={() => patch('enabled', !settings.enabled)}><span className="toggle-knob" /></button></label><span className="settings-hint">No permanent background polling. Refresh happens only while RD-Nexus is open.</span></div>
        <NumberSetting label="Default lookback (days)" value={settings.defaultLookbackDays} min={1} max={3650} onChange={(value) => patch('defaultLookbackDays', value)} />
        <NumberSetting label="Maximum messages per group" value={settings.defaultMaximumMessages} min={50} max={20000} onChange={(value) => patch('defaultMaximumMessages', value)} />
        <NumberSetting label="Complete archive safety bound" value={settings.defaultArchiveDepth} min={100} max={100000} onChange={(value) => patch('defaultArchiveDepth', value)} />
        <NumberSetting label="Foreground refresh interval (minutes)" value={settings.foregroundRefreshMinutes} min={5} max={1440} onChange={(value) => patch('foregroundRefreshMinutes', value)} />
        <NumberSetting label="Offline retention (days)" value={settings.offlineRetentionDays} min={7} max={3650} onChange={(value) => patch('offlineRetentionDays', value)} />
        <NumberSetting label="Attachment ceiling (MiB)" value={Math.round(settings.maximumAttachmentBytes / 1024 / 1024)} min={1} max={100} onChange={(value) => patch('maximumAttachmentBytes', value * 1024 * 1024)} />
        <NumberSetting label="Draft retention (days)" value={settings.draftRetentionDays} min={7} max={3650} onChange={(value) => patch('draftRetentionDays', value)} />
        <ToggleSetting label="Refresh on opening Groups.io" value={settings.refreshOnOpen} onChange={(value) => patch('refreshOnOpen', value)} />
        <ToggleSetting label="Auto-download attachments" value={settings.attachmentAutoDownload} onChange={(value) => patch('attachmentAutoDownload', value)} />
        <ToggleSetting label="Include muted groups in refresh" value={settings.includeMutedGroups} onChange={(value) => patch('includeMutedGroups', value)} />
        <ToggleSetting label="Show foreground new-message preview" value={settings.showNewMessagePreview} onChange={(value) => patch('showNewMessagePreview', value)} />
      </div>

      <section className="groupsio-settings-overrides" aria-labelledby="groupsio-overrides-heading">
        <div>
          <h3 id="groupsio-overrides-heading">Per-group storage policy</h3>
          <p className="settings-note">Override foreground sync, retention and attachment choices for individual memberships. Groups without an override inherit the defaults above.</p>
        </div>
        {status.groups.map((item) => {
          const explicit = settings.groupOverrides[item.id]
          const policy = { ...inheritedPolicy(), ...explicit }
          return <article key={item.id}>
            <div><strong>{item.title || item.name}</strong><small>{item.name} · last sync {item.lastSyncUnix ? new Date(item.lastSyncUnix * 1000).toLocaleString() : 'never'}</small></div>
            <label><span>Enabled</span><input type="checkbox" checked={policy.enabled} onChange={(event) => updateGroup(item.id, { enabled: event.target.checked })} /></label>
            <label><span>Inherit defaults</span><input type="checkbox" checked={policy.inheritGlobal} onChange={(event) => updateGroup(item.id, { inheritGlobal: event.target.checked })} /></label>
            {!policy.inheritGlobal && <>
              <label><span>Lookback days</span><input type="number" min={1} max={3650} value={policy.lookbackDays} onChange={(event) => updateGroup(item.id, { lookbackDays: Number(event.target.value) })} /></label>
              <label><span>Maximum messages</span><input type="number" min={50} max={20000} value={policy.maximumMessages} onChange={(event) => updateGroup(item.id, { maximumMessages: Number(event.target.value) })} /></label>
              <label><span>Attachments</span><input type="checkbox" checked={policy.attachments} onChange={(event) => updateGroup(item.id, { attachments: event.target.checked })} /></label>
              <label><span>Retention days</span><input type="number" min={7} max={3650} value={policy.retentionDays} onChange={(event) => updateGroup(item.id, { retentionDays: Number(event.target.value) })} /></label>
            </>}
            <button type="button" disabled={!explicit} onClick={() => resetGroup(item.id)}>Reset override</button>
          </article>
        })}
        {!status.groups.length && <p className="settings-note">Sync memberships to configure per-group policies.</p>}
      </section>

      <div className="settings-actions">
        <button type="button" className="primary" disabled={Boolean(busy)} onClick={() => void run('Groups.io settings save', () => saveGroupsIoSettings(settings))}>Save Groups.io settings</button>
        <button type="button" disabled={!status.connected || Boolean(busy)} onClick={() => void run('Groups.io sync', () => syncGroupsIo())}>Sync now</button>
        <button type="button" onClick={onOpenGroupsIo}>Open Groups.io screen</button>
        <button type="button" disabled={!status.connected || Boolean(busy)} onClick={() => void run('Disconnect', disconnectGroupsIo)}>Disconnect</button>
      </div>
      <p className="settings-note">Local storage: {size(status.storageBytes)} · API contract {status.apiRevision} · delivery policy {status.deliveryPolicy}</p>

      <div className="settings-danger-zone">
        <button type="button" onClick={() => void clear(false)}>Clear downloaded Groups.io cache</button>
        <button type="button" className="danger" onClick={() => void clear(true)}>Delete all local Groups.io data</button>
      </div>
    </>
  )
}

function NumberSetting({ label, value, min, max, onChange }: { label: string; value: number; min: number; max: number; onChange(value: number): void }) {
  return <div className="settings-field"><label className="settings-inline-label"><span className="settings-label">{label}</span><input className="settings-input" type="number" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.target.value))} /></label></div>
}

function ToggleSetting({ label, value, onChange }: { label: string; value: boolean; onChange(value: boolean): void }) {
  return <div className="settings-field"><label className="settings-toggle"><span className="settings-label">{label}</span><button type="button" role="switch" aria-checked={value} className={`toggle${value ? ' on' : ''}`} onClick={() => onChange(!value)}><span className="toggle-knob" /></button></label></div>
}
