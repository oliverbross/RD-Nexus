// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GroupsIoStatus } from '../api'
import { ConfirmHost } from '../confirm'
import { GroupsIoView } from './GroupsIoView'

const api = vi.hoisted(() => ({
  getStatus: vi.fn(),
  loadOlder: vi.fn(),
  loadMore: vi.fn(),
  incoming: vi.fn(),
  serverDrafts: vi.fn(),
  reconcile: vi.fn(),
  saveDraft: vi.fn(),
  sendDraft: vi.fn(),
}))

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getGroupsIoStatus: api.getStatus,
  loadOlderGroupsIoTopics: api.loadOlder,
  loadMoreGroupsIoMessages: api.loadMore,
  getGroupsIoIncomingAttachments: api.incoming,
  getGroupsIoServerDrafts: api.serverDrafts,
  reconcileGroupsIoDraft: api.reconcile,
  saveGroupsIoDraft: api.saveDraft,
  sendGroupsIoDraft: api.sendDraft,
  getGroupsIoDraftAttachments: vi.fn(async () => []),
}))

const settings = {
  enabled: true, defaultLookbackDays: 30, defaultMaximumMessages: 500,
  defaultArchiveDepth: 20_000, attachmentAutoDownload: false,
  maximumAttachmentBytes: 10 * 1024 * 1024, offlineRetentionDays: 180,
  refreshOnOpen: false, foregroundRefreshMinutes: 15, includeMutedGroups: false,
  draftRetentionDays: 90, showNewMessagePreview: false, mutedGroupIds: [], groupOverrides: {},
}

const status = (connected = true): GroupsIoStatus => ({
  enabled: true, connected, apiRevision: 'test', endpoint: 'https://groups.io/api/v1',
  providerState: connected ? 'READY' : 'CREDENTIAL_REQUIRED', deliveryPolicy: 'EXPLICIT_FOREGROUND_ONLY',
  counts: { groups: 1, topics: 1, messages: 1, drafts: 0, outbox: 0, files: 0 },
  storageBytes: 512, settings, lastSyncUnix: 1, newMessageIds: [],
  topicTokens: { g1: 'older' }, messageTokens: { t1: 'more' },
  archive: { state: 'paused', groupId: 'g1', downloaded: 50, total: null, rangeDays: 365, pageToken: 'next', message: '' },
  groups: [{ id: 'g1', name: 'nexus', title: 'Nexus Users', active: true, canPost: true, canReply: true, downloadArchives: true, lastSyncUnix: 1 }],
  topics: [{ id: 't1', groupId: 'g1', subject: 'Integration', messageCount: 1, lastMessageUnix: 1, closed: false }],
  messages: [{ id: 'g1:1', groupId: 'g1', topicId: 't1', messageNumber: '1', apiMessageId: 'm1', subject: 'Welcome', sender: 'K1ABC', body: 'Cached body', createdUnix: 1, attachmentCount: 1 }],
  drafts: [], outbox: [],
})

describe('GroupsIoView', () => {
  afterEach(cleanup)
  beforeEach(() => {
    vi.clearAllMocks()
    api.serverDrafts.mockResolvedValue({ items: [], nextPageToken: null })
    api.incoming.mockResolvedValue([])
    api.saveDraft.mockImplementation(async () => status(true))
  })

  it('renders the three-pane cached workspace while disconnected', async () => {
    api.getStatus.mockResolvedValue(status(false))
    render(<GroupsIoView onOpenSettings={() => {}} />)
    expect(await screen.findByText('Nexus Users')).toBeTruthy()
    expect(screen.getByText('Topics')).toBeTruthy()
    expect(await screen.findByText('Cached body')).toBeTruthy()
    expect(screen.getByText('API key required for live sync')).toBeTruthy()
  })

  it('shows a retryable load error instead of claiming credentials are missing', async () => {
    api.getStatus.mockRejectedValueOnce(new Error('native bridge unavailable')).mockResolvedValue(status(true))
    render(<GroupsIoView onOpenSettings={() => {}} />)
    expect((await screen.findAllByText('Groups.io could not be loaded')).length).toBe(2)
    expect(screen.queryByText('API key required for live sync')).toBeNull()
    fireEvent.click(screen.getByText('Retry'))
    expect(await screen.findByText('Nexus Users')).toBeTruthy()
  })

  it('wires pagination and incoming attachments without losing cached state', async () => {
    const next = status(true)
    api.getStatus.mockResolvedValue(next)
    api.loadOlder.mockResolvedValue(next)
    api.loadMore.mockResolvedValue(next)
    render(<GroupsIoView onOpenSettings={() => {}} />)
    await screen.findByText('Nexus Users')
    fireEvent.click(screen.getAllByText('Load older topics')[0])
    fireEvent.click(screen.getByText('Load more'))
    fireEvent.click(screen.getByText('1 attachments available through Groups.io'))
    await waitFor(() => expect(api.loadOlder).toHaveBeenCalledWith('g1', 'older'))
    expect(api.loadMore).toHaveBeenCalledWith('g1', 't1', 'more')
    expect(api.incoming).toHaveBeenCalledWith('g1', '1')
  })

  it('blocks editing an uncertain final post until explicit reconciliation', async () => {
    const next = status(true)
    next.drafts = [{
      id: 'd1', groupId: 'g1', topicId: null, replyMessageNumber: null,
      replyApiMessageId: null, remoteDraftId: 'remote-1', finalPostAttemptId: 'post-attempt-1',
      kind: 'new_topic', subject: 'Possibly delivered', body: 'Body', replyDestination: '',
      state: 'delivery_unknown', note: 'Reconcile first', updatedUnix: 1,
    }]
    api.getStatus.mockResolvedValue(next)
    render(<GroupsIoView onOpenSettings={() => {}} />)
    await screen.findByText('Nexus Users')
    fireEvent.click(screen.getByText('Drafts & outbox'))
    expect(await screen.findByText('Mark delivered')).toBeTruthy()
    expect(screen.getByText('Mark not delivered / allow retry')).toBeTruthy()
    expect(screen.getByText('Open').hasAttribute('disabled')).toBe(true)
  })

  it('refreshes native state after an ambiguous final post failure', async () => {
    const initial = status(true)
    const uncertain = status(true)
    uncertain.drafts = [{
      id: 'draft-new', groupId: 'g1', topicId: null, replyMessageNumber: null,
      replyApiMessageId: null, remoteDraftId: 'remote-1', finalPostAttemptId: 'post-1',
      kind: 'new_topic', subject: 'Possibly sent', body: 'Body', replyDestination: '',
      state: 'delivery_unknown', note: 'Reconcile first', updatedUnix: 2,
    }]
    api.getStatus.mockResolvedValueOnce(initial).mockResolvedValue(uncertain)
    api.saveDraft.mockResolvedValue(initial)
    api.sendDraft.mockRejectedValue('delivery unknown')
    render(<><GroupsIoView onOpenSettings={() => {}} /><ConfirmHost /></>)
    await screen.findByText('Nexus Users')
    fireEvent.click(screen.getByText('New topic'))
    fireEvent.change(screen.getByLabelText('Subject'), { target: { value: 'Possibly sent' } })
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'Body' } })
    fireEvent.click(screen.getByText('Review & send now'))
    fireEvent.click(await screen.findByText('Confirm'))
    await waitFor(() => expect(api.getStatus).toHaveBeenCalledTimes(2))
    fireEvent.click(screen.getByText('Drafts & outbox'))
    expect(await screen.findByText('Mark delivered')).toBeTruthy()
  })
})
