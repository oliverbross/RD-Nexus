import { useEffect, useMemo, useRef, useState } from 'react'
import {
  archiveGroupsIo,
  getGroupsIoStatus,
  saveGroupsIoDraft,
  saveGroupsIoSettings,
  searchGroupsIo,
  sendGroupsIoDraft,
  syncGroupsIo,
  type GroupsIoDraft,
  type GroupsIoMessage,
  type GroupsIoStatus,
} from '../api'
import { Dialog } from './ui/Dialog'
import { pushToast } from '../toast'
import './groups-io.css'

interface Props {
  onOpenSettings: () => void
}

type Composer = Pick<
  GroupsIoDraft,
  | 'id'
  | 'groupId'
  | 'topicId'
  | 'replyMessageNumber'
  | 'replyApiMessageId'
  | 'kind'
  | 'subject'
  | 'body'
  | 'replyDestination'
>

const uid = () => `groupsio-${Date.now()}-${Math.random().toString(36).slice(2)}`
const age = (unix: number) => unix > 0 ? new Date(unix * 1000).toLocaleString() : 'time unavailable'
const bytes = (value: number) => value < 1024 * 1024
  ? `${Math.max(0, Math.round(value / 1024))} KiB`
  : `${(value / 1024 / 1024).toFixed(1)} MiB`

function draftFrom(composer: Composer): GroupsIoDraft {
  return {
    ...composer,
    remoteDraftId: null,
    state: 'draft_local',
    note: '',
    updatedUnix: 0,
  }
}

export function GroupsIoView({ onOpenSettings }: Props) {
  const [status, setStatus] = useState<GroupsIoStatus | null>(null)
  const [selectedGroup, setSelectedGroup] = useState('')
  const [selectedTopic, setSelectedTopic] = useState('')
  const [query, setQuery] = useState('')
  const [online, setOnline] = useState(false)
  const [results, setResults] = useState<GroupsIoMessage[]>([])
  const [nextPage, setNextPage] = useState<string | null>(null)
  const [busy, setBusy] = useState('')
  const [composer, setComposer] = useState<Composer | null>(null)
  const [draftsOpen, setDraftsOpen] = useState(false)
  const [archiveOpen, setArchiveOpen] = useState(false)
  const [dismissedPreview, setDismissedPreview] = useState('')
  const refreshedOnOpen = useRef(false)

  const refresh = async () => {
    const next = await getGroupsIoStatus()
    setStatus(next)
    setSelectedGroup((current) => current || next.groups.find((group) => group.active)?.id || next.groups[0]?.id || '')
    return next
  }

  useEffect(() => { void refresh().catch((error) => pushToast(String(error), 'error')) }, [])
  useEffect(() => {
    if (refreshedOnOpen.current || !status?.connected || !status.enabled || !status.settings.refreshOnOpen) return
    const stale = !status.lastSyncUnix || Date.now() / 1000 - status.lastSyncUnix > status.settings.foregroundRefreshMinutes * 60
    if (!stale) return
    refreshedOnOpen.current = true
    void run('Refreshing Groups.io', () => syncGroupsIo())
  }, [status?.connected, status?.enabled])

  const groups = status?.groups ?? []
  const topics = useMemo(() => (status?.topics ?? []).filter((topic) => topic.groupId === selectedGroup), [status?.topics, selectedGroup])
  const messages = useMemo(() => (status?.messages ?? []).filter((message) => message.topicId === selectedTopic).sort((a, b) => a.createdUnix - b.createdUnix), [status?.messages, selectedTopic])
  const group = groups.find((item) => item.id === selectedGroup)
  const topic = topics.find((item) => item.id === selectedTopic)
  const newestMessage = status?.newMessageIds.map((id) => status.messages.find((message) => message.id === id)).filter((message): message is GroupsIoMessage => Boolean(message)).sort((a, b) => b.createdUnix - a.createdUnix)[0]

  useEffect(() => {
    if (!topics.some((item) => item.id === selectedTopic)) setSelectedTopic(topics[0]?.id ?? '')
  }, [selectedGroup, topics, selectedTopic])

  const run = async (label: string, action: () => Promise<GroupsIoStatus>) => {
    setBusy(label)
    try {
      const next = await action()
      setStatus(next)
      pushToast(`${label} complete`, 'success')
      return next
    } catch (error) {
      pushToast(typeof error === 'string' ? error : error instanceof Error ? error.message : `${label} failed`, 'error')
      return null
    } finally {
      setBusy('')
    }
  }

  const search = async (append = false) => {
    if (!query.trim()) { setResults([]); setNextPage(null); return }
    setBusy('Searching')
    try {
      const page = await searchGroupsIo(selectedGroup, query, online, append ? nextPage ?? undefined : undefined)
      setResults((current) => append ? [...current, ...page.items] : page.items)
      setNextPage(page.nextPageToken)
    } catch (error) {
      pushToast(String(error), 'error')
    } finally {
      setBusy('')
    }
  }

  const newTopic = () => {
    if (!selectedGroup) return
    setComposer({ id: uid(), groupId: selectedGroup, topicId: null, replyMessageNumber: null, replyApiMessageId: null, kind: 'new_topic', subject: '', body: '', replyDestination: '' })
  }

  const reply = (message: GroupsIoMessage) => setComposer({
    id: uid(), groupId: message.groupId, topicId: message.topicId,
    replyMessageNumber: message.messageNumber || null, replyApiMessageId: message.apiMessageId || null,
    kind: 'reply', subject: message.subject.startsWith('Re:') ? message.subject : `Re: ${message.subject}`,
    body: '', replyDestination: '',
  })

  const saveComposer = async () => {
    if (!composer) return
    const next = await run('Draft saved', () => saveGroupsIoDraft(draftFrom(composer)))
    if (next) setComposer(null)
  }

  const sendComposer = async () => {
    if (!composer) return
    const saved = await run('Draft saved', () => saveGroupsIoDraft(draftFrom(composer)))
    if (!saved) return
    if (!window.confirm(`Post “${composer.subject}” to ${group?.title || group?.name || 'this Groups.io group'} now?`)) return
    const sent = await run('Message submitted', () => sendGroupsIoDraft(composer.id, true))
    if (sent) setComposer(null)
  }

  const showResults = query.trim().length > 0
  return (
    <section className="panel groupsio-view" aria-label="Groups.io">
      <header className="groupsio-view-header">
        <div>
          <span className="eyebrow">MEMORIES / GROUPS.IO</span>
          <h2>Groups.io</h2>
          <p>{status?.connected ? 'Connected — memberships and downloaded messages are available here.' : 'Offline cache is available. Connect an API key in Settings for live sync.'}</p>
        </div>
        <div className="groupsio-header-actions">
          <button type="button" onClick={newTopic} disabled={!status?.connected || !group?.canPost}>New topic</button>
          <button type="button" onClick={() => setDraftsOpen(true)}>Drafts & outbox</button>
          <button type="button" onClick={() => setArchiveOpen(true)} disabled={!selectedGroup}>Offline archive</button>
          <button type="button" onClick={onOpenSettings}>Settings</button>
          <button type="button" className="primary" onClick={() => void run('Groups.io sync', () => syncGroupsIo())} disabled={!status?.connected || Boolean(busy)}>{busy || 'Sync all'}</button>
        </div>
        <div className="groupsio-summary" aria-live="polite">
          <strong>{status?.counts.groups ?? 0}</strong> groups · <strong>{status?.counts.topics ?? 0}</strong> topics · <strong>{status?.counts.messages ?? 0}</strong> messages · {bytes(status?.storageBytes ?? 0)}
        </div>
      </header>

      {!status?.connected && <div className="groupsio-boundary"><strong>API key required for live sync</strong><span>The key is held by your operating system credential vault, never this project or its config file.</span><button type="button" onClick={onOpenSettings}>Connect</button></div>}

      <div className="groupsio-searchbar">
        <div className="groupsio-segment" role="group" aria-label="Search location">
          <button type="button" aria-pressed={!online} onClick={() => { setOnline(false); setResults([]) }}>Downloaded</button>
          <button type="button" aria-pressed={online} onClick={() => { setOnline(true); setResults([]) }}>Groups.io</button>
        </div>
        <label><span>{online ? 'Search selected group online' : 'Search downloaded messages'}</span><input value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') void search() }} /></label>
        <button type="button" className="primary" onClick={() => void search()} disabled={!query.trim() || (online && (!selectedGroup || !status?.connected))}>Search</button>
      </div>

      {showResults ? (
        <div className="groupsio-results" role="list">
          {results.map((message) => <button type="button" role="listitem" key={message.id} onClick={() => { setSelectedGroup(message.groupId); setSelectedTopic(message.topicId); setQuery(''); setResults([]) }}><strong>{message.subject || 'Untitled message'}</strong><span>{message.sender} · {age(message.createdUnix)}</span><p>{message.body.slice(0, 360)}</p></button>)}
          {!results.length && <p className="empty">No matching messages.</p>}
          {nextPage && <button type="button" onClick={() => void search(true)}>Load more</button>}
        </div>
      ) : (
        <div className="groupsio-panes">
          <section className="groupsio-pane"><header><h3>Groups</h3></header><div className="groupsio-pane-scroll">{groups.map((item) => <button type="button" key={item.id} aria-current={item.id === selectedGroup ? 'true' : undefined} onClick={() => setSelectedGroup(item.id)}><i>{(item.title || item.name || 'G').slice(0, 2).toUpperCase()}</i><span><strong>{item.title || item.name}</strong><small>{item.active ? item.name : `${item.name} · cached`}</small></span></button>)}{!groups.length && <p className="empty">Connect and sync to download your memberships.</p>}</div></section>
          <section className="groupsio-pane"><header><h3>Topics</h3>{selectedGroup && <button type="button" onClick={() => void run('Group sync', () => syncGroupsIo(selectedGroup))} disabled={!status?.connected || Boolean(busy)}>Refresh</button>}</header><div className="groupsio-pane-scroll">{topics.map((item) => <button type="button" key={item.id} aria-current={item.id === selectedTopic ? 'true' : undefined} onClick={() => setSelectedTopic(item.id)}><span><strong>{item.subject || 'Untitled topic'}</strong><small>{item.messageCount} messages · {age(item.lastMessageUnix)}{item.closed ? ' · closed' : ''}</small></span></button>)}{selectedGroup && !topics.length && <p className="empty">No downloaded topics for this group.</p>}</div></section>
          <section className="groupsio-pane messages"><header><h3>{topic?.subject || 'Messages'}</h3></header><div className="groupsio-pane-scroll">{messages.map((message) => <article key={message.id} id={`groupsio-message-${message.id.replace(/:/g, '-')}`}><header><div><strong>{message.sender || 'Unknown sender'}</strong><small>{age(message.createdUnix)} · #{message.messageNumber}</small></div>{group?.canReply && !topic?.closed && <button type="button" onClick={() => reply(message)}>Reply</button>}</header><h4>{message.subject}</h4><p>{message.body}</p>{message.attachmentCount > 0 && <span className="groupsio-attachment">{message.attachmentCount} attachment{message.attachmentCount === 1 ? '' : 's'} available through Groups.io</span>}</article>)}{selectedTopic && !messages.length && <p className="empty">No downloaded messages in this topic.</p>}</div></section>
        </div>
      )}

      <Dialog open={composer !== null} onOpenChange={(open) => { if (!open) setComposer(null) }} title={composer?.kind === 'reply' ? 'Reply on Groups.io' : 'New Groups.io topic'} className="groupsio-dialog">
        {composer && <div className="groupsio-composer"><label><span>Subject</span><input value={composer.subject} onChange={(event) => setComposer({ ...composer, subject: event.target.value })} /></label>{composer.kind === 'reply' && <label><span>Reply destination</span><select value={composer.replyDestination} onChange={(event) => setComposer({ ...composer, replyDestination: event.target.value })}><option value="">Group</option><option value="sender">Sender</option><option value="group_and_sender">Group and sender</option><option value="mods">Moderators</option></select></label>}<label><span>Message</span><textarea rows={14} value={composer.body} onChange={(event) => setComposer({ ...composer, body: event.target.value })} /></label><p>Drafts save locally. Posting is explicit foreground work and is never retried after an uncertain delivery.</p><footer><button type="button" onClick={() => void saveComposer()}>Save draft</button><button type="button" className="primary" onClick={() => void sendComposer()} disabled={!composer.subject.trim() || !composer.body.trim() || Boolean(busy)}>Review & send now</button><button type="button" onClick={() => setComposer(null)}>Cancel</button></footer></div>}
      </Dialog>

      <Dialog open={draftsOpen} onOpenChange={setDraftsOpen} title="Groups.io drafts and outbox" className="groupsio-dialog wide">
        <div className="groupsio-drafts"><h3>Local drafts</h3>{status?.drafts.map((draft) => <article key={draft.id}><div><strong>{draft.subject || 'Untitled draft'}</strong><small>{draft.state} · {age(draft.updatedUnix)}</small></div><button type="button" onClick={() => { setComposer(draft); setDraftsOpen(false) }}>Open</button></article>)}{!status?.drafts.length && <p className="empty">No local drafts.</p>}<h3>Submission receipts</h3>{status?.outbox.map((draft) => <article key={draft.id}><div><strong>{draft.subject}</strong><small>{draft.state} · {draft.note}</small></div></article>)}{!status?.outbox.length && <p className="empty">No submission receipts.</p>}</div>
      </Dialog>

      <Dialog open={archiveOpen} onOpenChange={setArchiveOpen} title="Groups.io offline archive" className="groupsio-dialog">
        <div className="groupsio-archive"><p>Download bounded archive pages for the selected group. Completed pages remain after interruption and duplicate messages are replaced by identity.</p><dl><div><dt>State</dt><dd>{status?.archive.state ?? 'not started'}</dd></div><div><dt>Downloaded</dt><dd>{status?.archive.downloaded ?? 0}</dd></div><div><dt>Storage</dt><dd>{bytes(status?.storageBytes ?? 0)}</dd></div></dl><p>{status?.archive.message}</p><footer><button type="button" className="primary" disabled={!selectedGroup || !status?.connected || Boolean(busy)} onClick={() => void run('Archive sync', () => archiveGroupsIo(selectedGroup, 365))}>Sync past year</button><button type="button" disabled={!selectedGroup || !status?.connected || Boolean(busy)} onClick={() => void run('Archive sync', () => archiveGroupsIo(selectedGroup, null))}>Sync full history</button><button type="button" onClick={() => setArchiveOpen(false)}>Close</button></footer></div>
      </Dialog>

      {status?.settings.showNewMessagePreview && newestMessage && dismissedPreview !== newestMessage.id && <aside className="groupsio-new-preview" aria-live="polite">
        <span>NEW GROUPS.IO MESSAGE</span><strong>{newestMessage.subject || 'Untitled message'}</strong><small>{newestMessage.sender} · {age(newestMessage.createdUnix)}</small><p>{newestMessage.body.slice(0, 240)}</p>
        <div><button type="button" onClick={() => { setSelectedGroup(newestMessage.groupId); setSelectedTopic(newestMessage.topicId); setDismissedPreview(newestMessage.id) }}>Open</button><button type="button" onClick={() => setDismissedPreview(newestMessage.id)}>Dismiss</button><button type="button" onClick={() => void run('Group muted', () => saveGroupsIoSettings({ ...status.settings, mutedGroupIds: [...new Set([...status.settings.mutedGroupIds, newestMessage.groupId])] }))}>Mute group</button></div>
      </aside>}
    </section>
  )
}
