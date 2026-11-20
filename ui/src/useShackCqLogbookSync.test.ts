// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'
import { getShackCqPairing, syncShackCqSelectedLogbook } from './api'
import { useShackCqLogbookSync } from './useShackCqLogbookSync'

vi.mock('./api', () => ({
  getShackCqPairing: vi.fn(),
  syncShackCqSelectedLogbook: vi.fn(),
}))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

it('refreshes the selected authority after recovered outbox delivery', async () => {
  const pairing = { auth: { origin: 'http://127.0.0.1:4174' } }
  const result = {
    configuration: { logbook: { connectionKind: 'MANAGED' } },
    imported: { added: 0, skipped: 0, updated: 0, total: 5 },
    remoteCount: 5,
  }
  vi.mocked(getShackCqPairing).mockResolvedValue(pairing as Awaited<ReturnType<typeof getShackCqPairing>>)
  let finishFirst!: (value: Awaited<ReturnType<typeof syncShackCqSelectedLogbook>>) => void
  vi.mocked(syncShackCqSelectedLogbook)
    .mockReturnValueOnce(new Promise((resolve) => { finishFirst = resolve }))
    .mockResolvedValue(result as Awaited<ReturnType<typeof syncShackCqSelectedLogbook>>)

  renderHook(() => useShackCqLogbookSync(true))
  await waitFor(() => expect(syncShackCqSelectedLogbook).toHaveBeenCalledOnce())
  act(() => window.dispatchEvent(new Event('shackcq-contact-outbox-delivered')))
  finishFirst(result as Awaited<ReturnType<typeof syncShackCqSelectedLogbook>>)
  await waitFor(() => expect(syncShackCqSelectedLogbook).toHaveBeenCalledTimes(2))
})
