import { useEffect, useMemo, useRef, useState } from 'react'
import {
  addGroupsIoAttachment,
  archiveGroupsIo,
  deleteGroupsIoServerAttachment,
  deleteGroupsIoServerDraft,
  downloadGroupsIoIncomingAttachment,
  downloadGroupsIoOfficialArchive,
  getGroupsIoDraftAttachments,
  getGroupsIoIncomingAttachments,
  getGroupsIoServerDraftAttachments,
  getGroupsIoServerDrafts,
  getGroupsIoStatus,
  loadMoreGroupsIoMessages,
  loadOlderGroupsIoTopics,
  pauseGroupsIoArchive,
  queueGroupsIoDraft,
  reconcileGroupsIoDraft,
  removeGroupsIoAttachment,
  saveGroupsIoDraft,
  saveGroupsIoSettings,
  searchGroupsIo,
  sendGroupsIoDraft,
  syncGroupsIo,
  type GroupsIoDraft,
  type GroupsIoAttachment,
  type GroupsIoFilePayload,
  type GroupsIoMessage,
  type GroupsIoRemoteAttachment,
  type GroupsIoServerDraft,
  type GroupsIoStatus,
} from '../api'
import { t } from '../i18n'
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
const age = (unix: number) => unix > 0 ? new Date(unix * 1000).toLocaleString() : t('groupsio.time.unavailable')
const bytes = (value: number) => value < 1024 * 1024
  ? `${Math.max(0, Math.round(value / 1024))} KiB`
  : `${(value / 1024 / 1024).toFixed(1)} MiB`

function deliverFile(file: GroupsIoFilePayload, preview = false) {
  const url = URL.createObjectURL(new Blob([new Uint8Array(file.bytes)], { type: file.mediaType }))
  if (preview) window.open(url, '_blank', 'noopener,noreferrer')
  else {
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = file.filename
    anchor.click()
  }
  window.setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

function draftFrom(composer: Composer): GroupsIoDraft {
  return {
    ...composer,
    remoteDraftId: null,
    finalPostAttemptId: null,
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
  const [incomingFor, setIncomingFor] = useState<GroupsIoMessage | null>(null)
  const [incoming, setIncoming] = useState<GroupsIoRemoteAttachment[]>([])
  const [attachments, setAttachments] = useState<GroupsIoAttachment[]>([])
  const [serverDrafts, setServerDrafts] = useState<GroupsIoServerDraft[]>([])
  const [serverNext, setServerNext] = useState<string | null>(null)
  const [serverAttachments, setServerAttachments] = useState<{ draftId: string; items: GroupsIoRemoteAttachment[] } | null>(null)
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
    void run(t('groupsio.action.refreshing'), () => syncGroupsIo())
  }, [status?.connected, status?.enabled])
  useEffect(() => {
    if (!status?.connected || !status.enabled) return
    const interval = window.setInterval(() => {
      if (!busy) void run(t('groupsio.action.scheduledRefresh'), () => syncGroupsIo())
    }, status.settings.foregroundRefreshMinutes * 60_000)
    return () => window.clearInterval(interval)
  }, [status?.connected, status?.enabled, status?.settings.foregroundRefreshMinutes, busy])

  const groups = status?.groups ?? []
  const topics = useMemo(() => (status?.topics ?? []).filter((topic) => topic.groupId === selectedGroup), [status?.topics, selectedGroup])
  const messages = useMemo(() => (status?.messages ?? []).filter((message) => message.topicId === selectedTopic).sort((a, b) => a.createdUnix - b.createdUnix), [status?.messages, selectedTopic])
  const group = groups.find((item) => item.id === selectedGroup)
  const topic = topics.find((item) => item.id === selectedTopic)
  const newestMessage = status?.newMessageIds.map((id) => status.messages.find((message) => message.id === id)).filter((message): message is GroupsIoMessage => Boolean(message)).sort((a, b) => b.createdUnix - a.createdUnix)[0]

  useEffect(() => {
    if (!topics.some((item) => item.id === selectedTopic)) setSelectedTopic(topics[0]?.id ?? '')
  }, [selectedGroup, topics, selectedTopic])
  useEffect(() => {
    if (!composer) { setAttachments([]); return }
    void getGroupsIoDraftAttachments(composer.id).then(setAttachments).catch(() => setAttachments([]))
  }, [composer?.id])
  useEffect(() => {
    if (!incomingFor) { setIncoming([]); return }
    void getGroupsIoIncomingAttachments(incomingFor.groupId, incomingFor.messageNumber)
      .then(setIncoming)
      .catch((error) => pushToast(String(error), 'error'))
  }, [incomingFor?.id])

  const run = async (label: string, action: () => Promise<GroupsIoStatus>) => {
    setBusy(label)
    try {
      const next = await action()
      setStatus(next)
      pushToast(t('groupsio.action.complete', { label }), 'success')
      return next
    } catch (error) {
      // Native state may have advanced even when an operation returns an error (notably an
      // ambiguous final post). Refresh immediately so reconciliation controls are never hidden
      // behind stale pre-send state.
      try { setStatus(await getGroupsIoStatus()) } catch { /* preserve the actionable error */ }
      pushToast(typeof error === 'string' ? error : error instanceof Error ? error.message : t('groupsio.action.failed', { label }), 'error')
      return null
    } finally {
      setBusy('')
    }
  }

  const search = async (append = false) => {
    if (!query.trim()) { setResults([]); setNextPage(null); return }
    setBusy(t('groupsio.action.searching'))
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
    const next = await run(t('groupsio.action.draftSaved'), () => saveGroupsIoDraft(draftFrom(composer)))
    if (next) setComposer(null)
  }

  const sendComposer = async () => {
    if (!composer) return
    const saved = await run(t('groupsio.action.draftSaved'), () => saveGroupsIoDraft(draftFrom(composer)))
    if (!saved) return
    if (!window.confirm(t('groupsio.composer.confirm', { subject: composer.subject, group: group?.title || group?.name || t('groupsio.thisGroup') }))) return
    const sent = await run(t('groupsio.action.submitted'), () => sendGroupsIoDraft(composer.id, true))
    if (sent) setComposer(null)
  }

  const queueComposer = async () => {
    if (!composer) return
    const saved = await run(t('groupsio.action.draftSaved'), () => saveGroupsIoDraft(draftFrom(composer)))
    if (!saved) return
    const queued = await run(t('groupsio.action.queued'), () => queueGroupsIoDraft(composer.id))
    if (queued) setComposer(null)
  }

  const attachFiles = async (files: FileList | null) => {
    if (!composer || !files?.length) return
    const saved = await run(t('groupsio.action.draftSaved'), () => saveGroupsIoDraft(draftFrom(composer)))
    if (!saved) return
    setBusy(t('groupsio.action.addingAttachments'))
    try {
      let next = attachments
      for (const file of Array.from(files)) {
        if (file.size > saved.settings.maximumAttachmentBytes) throw new Error(t('groupsio.attachment.tooLarge', { size: bytes(saved.settings.maximumAttachmentBytes) }))
        next = await addGroupsIoAttachment(composer.id, file.name, file.type || 'application/octet-stream', Array.from(new Uint8Array(await file.arrayBuffer())))
      }
      setAttachments(next)
    } catch (error) { pushToast(String(error), 'error') }
    finally { setBusy('') }
  }

  const refreshServerDrafts = async (append = false) => {
    setBusy(t('groupsio.action.refreshingServerDrafts'))
    try {
      const page = await getGroupsIoServerDrafts(append ? serverNext ?? undefined : undefined)
      setServerDrafts((current) => append ? [...current, ...page.items] : page.items)
      setServerNext(page.nextPageToken)
    } catch (error) { pushToast(String(error), 'error') }
    finally { setBusy('') }
  }

  const openServerDraft = (draft: GroupsIoServerDraft) => {
    setComposer({
      id: uid(), groupId: draft.groupId, topicId: null,
      replyMessageNumber: null, replyApiMessageId: draft.messageId || null,
      kind: draft.kind.includes('reply') ? 'reply' : 'new_topic',
      subject: draft.subject, body: draft.body, replyDestination: '',
    })
    setDraftsOpen(false)
  }

  const showResults = query.trim().length > 0
  return (
    <section className="panel groupsio-view" aria-label={t('groupsio.title')}>
      <header className="groupsio-view-header">
        <div className="groupsio-heading">
          <h2>{t('groupsio.title')}</h2>
          <span className={`groupsio-connection${status?.connected ? ' connected' : ''}`} role="status">
            {status?.connected ? t('groupsio.connected') : t('groupsio.offline')}
          </span>
        </div>
        <div className="groupsio-header-actions">
          <button type="button" onClick={newTopic} disabled={!status?.connected || !group?.canPost}>{t('groupsio.newTopic')}</button>
          <button type="button" onClick={() => { setDraftsOpen(true); void refreshServerDrafts() }}>{t('groupsio.draftsOutbox')}</button>
          <button type="button" onClick={() => setArchiveOpen(true)} disabled={!selectedGroup}>{t('groupsio.archive.title')}</button>
          <button type="button" onClick={onOpenSettings}>{t('groupsio.settings')}</button>
          <button type="button" className="primary" onClick={() => void run(t('groupsio.action.sync'), () => syncGroupsIo())} disabled={!status?.connected || Boolean(busy)}>{busy || t('groupsio.syncAll')}</button>
        </div>
        <div className="groupsio-summary" aria-live="polite">
          {t('groupsio.summary', { groups: status?.counts.groups ?? 0, topics: status?.counts.topics ?? 0, messages: status?.counts.messages ?? 0, size: bytes(status?.storageBytes ?? 0) })}
        </div>
      </header>

      {!status?.connected && <div className="groupsio-boundary"><strong>{t('groupsio.keyRequired')}</strong><span>{t('groupsio.keyBoundary')}</span><button type="button" onClick={onOpenSettings}>{t('groupsio.connect')}</button></div>}

      <div className="groupsio-searchbar">
        <div className="groupsio-segment" role="group" aria-label={t('groupsio.search.location')}>
          <button type="button" aria-pressed={!online} onClick={() => { setOnline(false); setResults([]) }}>{t('groupsio.search.downloaded')}</button>
          <button type="button" aria-pressed={online} onClick={() => { setOnline(true); setResults([]) }}>{t('groupsio.title')}</button>
        </div>
        <label><span>{online ? t('groupsio.search.online') : t('groupsio.search.local')}</span><input value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') void search() }} /></label>
        <button type="button" className="primary" onClick={() => void search()} disabled={!query.trim() || (online && (!selectedGroup || !status?.connected))}>{t('groupsio.search.action')}</button>
      </div>

      {showResults ? (
        <div className="groupsio-results" role="list">
          {results.map((message) => <button type="button" role="listitem" key={message.id} onClick={() => { setSelectedGroup(message.groupId); setSelectedTopic(message.topicId); setQuery(''); setResults([]) }}><strong>{message.subject || t('groupsio.untitledMessage')}</strong><span>{message.sender} · {age(message.createdUnix)}</span><p>{message.body.slice(0, 360)}</p></button>)}
          {!results.length && <p className="empty">{t('groupsio.search.none')}</p>}
          {nextPage && <button type="button" onClick={() => void search(true)}>{t('groupsio.loadMore')}</button>}
        </div>
      ) : (
        <div className="groupsio-panes">
          <section className="groupsio-pane"><header><h3>{t('groupsio.groups')}</h3></header><div className="groupsio-pane-scroll">{groups.map((item) => <button type="button" key={item.id} aria-current={item.id === selectedGroup ? 'true' : undefined} onClick={() => setSelectedGroup(item.id)}><i>{(item.title || item.name || 'G').slice(0, 2).toUpperCase()}</i><span><strong>{item.title || item.name}</strong><small>{item.active ? item.name : t('groupsio.cachedGroup', { name: item.name })}</small></span></button>)}{!groups.length && <p className="empty">{t('groupsio.groups.empty')}</p>}</div></section>
          <section className="groupsio-pane"><header><h3>{t('groupsio.topics')}</h3>{selectedGroup && <button type="button" onClick={() => void run(t('groupsio.action.groupSync'), () => syncGroupsIo(selectedGroup))} disabled={!status?.connected || Boolean(busy)}>{t('groupsio.refresh')}</button>}</header><div className="groupsio-pane-scroll">{topics.map((item) => <button type="button" key={item.id} aria-current={item.id === selectedTopic ? 'true' : undefined} onClick={() => setSelectedTopic(item.id)}><span><strong>{item.subject || t('groupsio.untitledTopic')}</strong><small>{t('groupsio.topicMeta', { count: item.messageCount, time: age(item.lastMessageUnix), closed: item.closed ? t('groupsio.closedSuffix') : '' })}</small></span></button>)}{selectedGroup && !topics.length && <p className="empty">{t('groupsio.topics.empty')}</p>}{selectedGroup && status?.topicTokens[selectedGroup] && <button type="button" onClick={() => void run(t('groupsio.action.loadOlder'), () => loadOlderGroupsIoTopics(selectedGroup, status.topicTokens[selectedGroup]))}>{t('groupsio.loadOlder')}</button>}</div></section>
          <section className="groupsio-pane messages"><header><h3>{topic?.subject || t('groupsio.messages')}</h3></header><div className="groupsio-pane-scroll">{messages.map((message) => <article key={message.id} id={`groupsio-message-${message.id.replace(/:/g, '-')}`}><header><div><strong>{message.sender || t('groupsio.unknownSender')}</strong><small>{age(message.createdUnix)} · #{message.messageNumber}</small></div>{group?.canReply && !topic?.closed && <button type="button" onClick={() => reply(message)}>{t('groupsio.reply')}</button>}</header><h4>{message.subject}</h4><p>{message.body}</p>{message.attachmentCount > 0 && <button type="button" className="groupsio-attachment" onClick={() => setIncomingFor(message)}>{t('groupsio.attachments.available', { count: message.attachmentCount })}</button>}</article>)}{selectedTopic && !messages.length && <p className="empty">{t('groupsio.messages.empty')}</p>}{selectedGroup && selectedTopic && status?.messageTokens[selectedTopic] && <button type="button" onClick={() => void run(t('groupsio.action.loadMoreMessages'), () => loadMoreGroupsIoMessages(selectedGroup, selectedTopic, status.messageTokens[selectedTopic]))}>{t('groupsio.loadMore')}</button>}</div></section>
        </div>
      )}

      <Dialog open={composer !== null} onOpenChange={(open) => { if (!open) setComposer(null) }} title={composer?.kind === 'reply' ? t('groupsio.composer.replyTitle') : t('groupsio.composer.newTitle')} description={t('groupsio.composer.policy')} className="groupsio-dialog">
        {composer && <div className="groupsio-composer"><label><span>{t('groupsio.composer.subject')}</span><input value={composer.subject} onChange={(event) => setComposer({ ...composer, subject: event.target.value })} /></label>{composer.kind === 'reply' && <label><span>{t('groupsio.composer.destination')}</span><select value={composer.replyDestination} onChange={(event) => setComposer({ ...composer, replyDestination: event.target.value })}><option value="">{t('groupsio.groups')}</option><option value="sender">{t('groupsio.composer.sender')}</option><option value="group_and_sender">{t('groupsio.composer.groupSender')}</option><option value="mods">{t('groupsio.composer.moderators')}</option></select></label>}<label><span>{t('groupsio.composer.message')}</span><textarea rows={14} value={composer.body} onChange={(event) => setComposer({ ...composer, body: event.target.value })} /></label><label><span>{t('groupsio.composer.attach')}</span><input type="file" multiple onChange={(event) => void attachFiles(event.target.files)} /></label>{attachments.map((item) => <div className="groupsio-file" key={item.id}><span>{item.filename} · {bytes(item.byteSize)}</span><button type="button" onClick={() => void removeGroupsIoAttachment(composer.id, item.id).then(setAttachments)}>{t('groupsio.remove')}</button></div>)}<p>{t('groupsio.composer.policy')}</p><footer><button type="button" onClick={() => void saveComposer()}>{t('groupsio.composer.save')}</button><button type="button" onClick={() => void queueComposer()} disabled={!composer.subject.trim() || !composer.body.trim() || Boolean(busy)}>{t('groupsio.composer.queue')}</button><button type="button" className="primary" onClick={() => void sendComposer()} disabled={!composer.subject.trim() || !composer.body.trim() || Boolean(busy)}>{t('groupsio.composer.send')}</button><button type="button" onClick={() => setComposer(null)}>{t('groupsio.cancel')}</button></footer></div>}
      </Dialog>

      <Dialog open={draftsOpen} onOpenChange={setDraftsOpen} title={t('groupsio.drafts.title')} description={t('groupsio.drafts.description')} className="groupsio-dialog wide">
        <div className="groupsio-drafts"><h3>{t('groupsio.drafts.local')}</h3>{status?.drafts.map((draft) => <article key={draft.id}><div><strong>{draft.subject || t('groupsio.drafts.untitled')}</strong><small>{draft.state} · {age(draft.updatedUnix)}</small>{draft.finalPostAttemptId && <small>{t('groupsio.drafts.attempt', { id: draft.finalPostAttemptId })}</small>}</div><div className="groupsio-inline-actions"><button type="button" onClick={() => { setComposer(draft); setDraftsOpen(false) }} disabled={draft.state === 'posting' || draft.state === 'delivery_unknown'}>{t('groupsio.open')}</button>{(draft.state === 'posting' || draft.state === 'delivery_unknown') && <><button type="button" onClick={() => { if (window.confirm(t('groupsio.drafts.markDeliveredConfirm'))) void run(t('groupsio.action.reconciled'), () => reconcileGroupsIoDraft(draft.id, 'delivered')) }}>{t('groupsio.drafts.markDelivered')}</button><button type="button" onClick={() => { if (window.confirm(t('groupsio.drafts.retryConfirm'))) void run(t('groupsio.action.reconciled'), () => reconcileGroupsIoDraft(draft.id, 'not_delivered')) }}>{t('groupsio.drafts.allowRetry')}</button></>}</div></article>)}{!status?.drafts.length && <p className="empty">{t('groupsio.drafts.none')}</p>}<h3>{t('groupsio.drafts.receipts')}</h3>{status?.outbox.map((draft) => <article key={draft.id}><div><strong>{draft.subject}</strong><small>{draft.state} · {draft.note}</small></div></article>)}{!status?.outbox.length && <p className="empty">{t('groupsio.drafts.noReceipts')}</p>}<h3>{t('groupsio.drafts.server')}</h3><button type="button" onClick={() => void refreshServerDrafts()}>{t('groupsio.refresh')}</button>{serverDrafts.map((draft) => <article key={draft.id}><div><strong>{draft.subject || t('groupsio.drafts.untitled')}</strong><small>{age(draft.updatedUnix)} · {t('groupsio.attachments.count', { count: draft.attachmentCount })}</small></div><div className="groupsio-inline-actions"><button type="button" onClick={() => openServerDraft(draft)}>{t('groupsio.drafts.copyLocal')}</button>{draft.attachmentCount > 0 && <button type="button" onClick={() => void getGroupsIoServerDraftAttachments(draft.id).then((items) => setServerAttachments({ draftId: draft.id, items }))}>{t('groupsio.attachments.action')}</button>}<button type="button" onClick={() => { if (window.confirm(t('groupsio.drafts.deleteConfirm', { subject: draft.subject || t('groupsio.drafts.untitled') }))) void deleteGroupsIoServerDraft(draft.id).then(() => refreshServerDrafts()) }}>{t('groupsio.delete')}</button></div></article>)}{serverNext && <button type="button" onClick={() => void refreshServerDrafts(true)}>{t('groupsio.loadMore')}</button>}{serverAttachments && <div className="groupsio-server-files">{serverAttachments.items.map((item) => <span key={item.id}>{item.filename} · {bytes(item.byteSize)} <button type="button" onClick={() => { if (window.confirm(t('groupsio.attachments.deleteConfirm', { filename: item.filename }))) void deleteGroupsIoServerAttachment(serverAttachments.draftId, item.id).then(async () => setServerAttachments({ draftId: serverAttachments.draftId, items: await getGroupsIoServerDraftAttachments(serverAttachments.draftId) })) }}>{t('groupsio.delete')}</button></span>)}</div>}</div>
      </Dialog>

      <Dialog open={archiveOpen} onOpenChange={setArchiveOpen} title={t('groupsio.archive.title')} description={t('groupsio.archive.help')} className="groupsio-dialog">
        <div className="groupsio-archive"><p>{t('groupsio.archive.help')}</p><dl><div><dt>{t('groupsio.archive.state')}</dt><dd>{status?.archive.state ?? t('groupsio.archive.notStarted')}</dd></div><div><dt>{t('groupsio.archive.downloaded')}</dt><dd>{status?.archive.downloaded ?? 0}</dd></div><div><dt>{t('groupsio.archive.storage')}</dt><dd>{bytes(status?.storageBytes ?? 0)}</dd></div></dl><p>{status?.archive.message}</p><footer><button type="button" className="primary" disabled={!selectedGroup || !status?.connected || Boolean(busy)} onClick={() => void run(t('groupsio.action.archiveSync'), () => archiveGroupsIo(selectedGroup, 365))}>{t('groupsio.archive.year')}</button><button type="button" disabled={!selectedGroup || !status?.connected || Boolean(busy)} onClick={() => void run(t('groupsio.action.archiveSync'), () => archiveGroupsIo(selectedGroup, null))}>{t('groupsio.archive.full')}</button><button type="button" onClick={() => void run(t('groupsio.action.archivePause'), pauseGroupsIoArchive)}>{t('groupsio.archive.pause')}</button>{group?.downloadArchives && <button type="button" onClick={() => void downloadGroupsIoOfficialArchive(selectedGroup).then((file) => deliverFile(file))}>{t('groupsio.archive.official')}</button>}<button type="button" onClick={() => setArchiveOpen(false)}>{t('groupsio.close')}</button></footer></div>
      </Dialog>

      <Dialog open={incomingFor !== null} onOpenChange={(open) => { if (!open) setIncomingFor(null) }} title={t('groupsio.attachments.title')} description={t('groupsio.attachments.description')} className="groupsio-dialog">
        <div className="groupsio-drafts">{incoming.map((item) => <article key={item.id}><div><strong>{item.filename}</strong><small>{bytes(item.byteSize)}{item.cached ? ` · ${t('groupsio.attachments.cached')}` : ''}</small></div><div className="groupsio-inline-actions"><button type="button" onClick={() => incomingFor && void downloadGroupsIoIncomingAttachment(incomingFor.groupId, incomingFor.messageNumber, item.id, true).then((file) => deliverFile(file, true))}>{t('groupsio.attachments.preview')}</button><button type="button" onClick={() => incomingFor && void downloadGroupsIoIncomingAttachment(incomingFor.groupId, incomingFor.messageNumber, item.id, false).then((file) => deliverFile(file))}>{t('groupsio.attachments.download')}</button></div></article>)}{!incoming.length && <p className="empty">{t('groupsio.attachments.none')}</p>}</div>
      </Dialog>

      {status?.settings.showNewMessagePreview && newestMessage && dismissedPreview !== newestMessage.id && <aside className="groupsio-new-preview" aria-live="polite">
        <span>{t('groupsio.preview.new')}</span><strong>{newestMessage.subject || t('groupsio.untitledMessage')}</strong><small>{newestMessage.sender} · {age(newestMessage.createdUnix)}</small><p>{newestMessage.body.slice(0, 240)}</p>
        <div><button type="button" onClick={() => { setSelectedGroup(newestMessage.groupId); setSelectedTopic(newestMessage.topicId); setDismissedPreview(newestMessage.id) }}>{t('groupsio.open')}</button><button type="button" onClick={() => setDismissedPreview(newestMessage.id)}>{t('groupsio.dismiss')}</button><button type="button" onClick={() => void run(t('groupsio.action.muted'), () => saveGroupsIoSettings({ ...status.settings, mutedGroupIds: [...new Set([...status.settings.mutedGroupIds, newestMessage.groupId])] }))}>{t('groupsio.mute')}</button></div>
      </aside>}
    </section>
  )
}
