// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GroupsIoStatus } from '../api'
import { GroupsIoView } from './GroupsIoView'

const api = vi.hoisted(() => ({
  getStatus: vi.fn(),
  loadOlder: vi.fn(),
  loadMore: vi.fn(),
  incoming: vi.fn(),
  serverDrafts: vi.fn(),
}))

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getGroupsIoStatus: api.getStatus,
  loadOlderGroupsIoTopics: api.loadOlder,
  loadMoreGroupsIoMessages: api.loadMore,
  getGroupsIoIncomingAttachments: api.incoming,
  getGroupsIoServerDrafts: api.serverDrafts,
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
  })

  it('renders the three-pane cached workspace while disconnected', async () => {
    api.getStatus.mockResolvedValue(status(false))
    render(<GroupsIoView onOpenSettings={() => {}} />)
    expect(await screen.findByText('Nexus Users')).toBeTruthy()
    expect(screen.getByText('Topics')).toBeTruthy()
    expect(await screen.findByText('Cached body')).toBeTruthy()
    expect(screen.getByText('API key required for live sync')).toBeTruthy()
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
})
