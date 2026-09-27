// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { GroupsIoStatus } from '../api'
import { GroupsIoSettings } from './GroupsIoSettings'

const api = vi.hoisted(() => ({ status: vi.fn(), save: vi.fn() }))
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getGroupsIoStatus: api.status,
  saveGroupsIoSettings: api.save,
}))

const fixture: GroupsIoStatus = {
  enabled: true, connected: true, apiRevision: 'test', endpoint: 'https://groups.io/api/v1',
  providerState: 'READY', deliveryPolicy: 'EXPLICIT_FOREGROUND_ONLY',
  counts: { groups: 1, topics: 0, messages: 0, drafts: 0, outbox: 0, files: 0 }, storageBytes: 10,
  settings: { enabled: true, defaultLookbackDays: 30, defaultMaximumMessages: 500, defaultArchiveDepth: 20000, attachmentAutoDownload: false, maximumAttachmentBytes: 10485760, offlineRetentionDays: 180, refreshOnOpen: true, foregroundRefreshMinutes: 15, includeMutedGroups: false, draftRetentionDays: 90, showNewMessagePreview: true, mutedGroupIds: [], groupOverrides: {} },
  lastSyncUnix: 0, newMessageIds: [], topicTokens: {}, messageTokens: {},
  archive: { state: 'idle', groupId: null, downloaded: 0, total: null, rangeDays: null, pageToken: null, message: '' },
  groups: [{ id: 'g1', name: 'nexus', title: 'Nexus Users', active: true, canPost: true, canReply: true, downloadArchives: true, lastSyncUnix: 0 }],
  topics: [], messages: [], drafts: [], outbox: [],
}

describe('GroupsIoSettings', () => {
  afterEach(cleanup)
  it('never renders the stored key and applies a bulk group policy', async () => {
    api.status.mockResolvedValue(fixture)
    api.save.mockResolvedValue(fixture)
    render(<GroupsIoSettings onOpenGroupsIo={() => {}} />)
    expect(await screen.findByText('Nexus Users')).toBeTruthy()
    expect(screen.queryByDisplayValue(/api|secret|bearer/i)).toBeNull()
    fireEvent.click(screen.getByLabelText('Select Nexus Users for bulk policy changes'))
    fireEvent.click(screen.getByText('Apply defaults to selected'))
    fireEvent.click(screen.getByText('Save Groups.io settings'))
    await waitFor(() => expect(api.save).toHaveBeenCalled())
    expect(api.save.mock.calls[0][0].groupOverrides.g1).toMatchObject({ inheritGlobal: false, lookbackDays: 30 })
  })
})
