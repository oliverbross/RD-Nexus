// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { ShackCqPairing } from './ShackCqPairing'
import { exportShackCqUnassignedContacts, flushShackCqContactOutbox, getShackCqContactOutboxStatus, getShackCqPairing, retryShackCqRejectedContacts } from '../api'

vi.mock('../api', () => ({
  getShackCqPairing: vi.fn(async () => null),
  getShackCqAgentStatus: vi.fn(async () => ({ connected: false })),
  exportShackCqUnassignedContacts: vi.fn(async () => '<CALL:6>N0TEST<EOR>'),
  forgetShackCqPairing: vi.fn(), flushShackCqContactOutbox: vi.fn(),
  getShackCqContactOutboxStatus: vi.fn(async () => ({ pending: 0, rejected: 0, delivered: 0, awaitingDestination: 0, lastError: null })),
  retryShackCqRejectedContacts: vi.fn(),
  pairShackCq: vi.fn(), startShackCqAgent: vi.fn(), stopShackCqAgent: vi.fn(),
}))
afterEach(() => { cleanup(); vi.clearAllMocks(); vi.restoreAllMocks() })

it('exports recoverable unassigned contacts without pairing or assigning an authority', async () => {
  render(<ShackCqPairing mode="basic" />)
  fireEvent.click(await screen.findByRole('button', { name: 'Export unassigned contacts' }))
  await waitFor(() => expect((screen.getByRole('textbox', { name: 'Unassigned contacts ADIF' }) as HTMLTextAreaElement).value).toBe('<CALL:6>N0TEST<EOR>'))
  expect(exportShackCqUnassignedContacts).toHaveBeenCalledOnce()
})

it('requires an explicit gesture to retry retained contacts and reports a refused retry', async () => {
  vi.mocked(getShackCqPairing).mockResolvedValueOnce({ stationLabel: 'Test station', accountLabel: 'Test account' } as Awaited<ReturnType<typeof getShackCqPairing>>)
  vi.mocked(flushShackCqContactOutbox).mockResolvedValue({ pending: 1, rejected: 1, delivered: 0, awaitingDestination: 0, lastError: null })
  vi.mocked(retryShackCqRejectedContacts).mockRejectedValueOnce('accessDenied')
  render(<ShackCqPairing mode="basic" />)
  const retry = await screen.findByRole('button', { name: 'Retry retained contacts' })
  expect(retryShackCqRejectedContacts).not.toHaveBeenCalled()
  fireEvent.click(retry)
  await screen.findByText('ShackCQ operation unavailable · accessDenied')
  expect(retryShackCqRejectedContacts).toHaveBeenCalledOnce()
  expect(screen.getByText(/1 contact\(s\) need recovery/)).toBeTruthy()
})

it('shows the durable local pending count when offline delivery fails after relaunch', async () => {
  vi.mocked(getShackCqPairing).mockResolvedValueOnce({ stationLabel: 'Test station', accountLabel: 'Test account' } as Awaited<ReturnType<typeof getShackCqPairing>>)
  vi.mocked(getShackCqContactOutboxStatus).mockResolvedValueOnce({ pending: 1, rejected: 0, delivered: 0, awaitingDestination: 0, lastError: null })
  vi.mocked(flushShackCqContactOutbox).mockRejectedValueOnce('serviceUnavailable')
  render(<ShackCqPairing mode="digi" />)
  await screen.findByText(/1 contact pending/)
  expect(screen.queryByText(/contact delivery caught up/)).toBeNull()
  await waitFor(() => expect(flushShackCqContactOutbox).toHaveBeenCalledOnce())
})

it('requests an authority refresh after normal recovery delivers a contact', async () => {
  vi.mocked(getShackCqPairing).mockResolvedValueOnce({ stationLabel: 'Test station', accountLabel: 'Test account' } as Awaited<ReturnType<typeof getShackCqPairing>>)
  vi.mocked(flushShackCqContactOutbox).mockResolvedValueOnce({ pending: 0, rejected: 0, delivered: 1, awaitingDestination: 0, lastError: null })
  const delivered = vi.fn()
  window.addEventListener('shackcq-contact-outbox-delivered', delivered)
  render(<ShackCqPairing mode="digi" />)
  await waitFor(() => expect(delivered).toHaveBeenCalledOnce())
  window.removeEventListener('shackcq-contact-outbox-delivered', delivered)
})

it('updates retained contact status only after the retry receipt', async () => {
  vi.mocked(getShackCqPairing).mockResolvedValueOnce({ stationLabel: 'Test station', accountLabel: 'Test account' } as Awaited<ReturnType<typeof getShackCqPairing>>)
  vi.mocked(flushShackCqContactOutbox).mockResolvedValue({ pending: 1, rejected: 1, delivered: 0, awaitingDestination: 0, lastError: null })
  let finish!: (status: Awaited<ReturnType<typeof retryShackCqRejectedContacts>>) => void
  vi.mocked(retryShackCqRejectedContacts).mockReturnValueOnce(new Promise((resolve) => { finish = resolve }))
  render(<ShackCqPairing mode="digi" />)
  const retry = await screen.findByRole('button', { name: 'Retry retained contacts' })
  fireEvent.click(retry)
  expect((retry as HTMLButtonElement).disabled).toBe(true)
  expect(screen.getByText(/1 contact\(s\) need recovery/)).toBeTruthy()
  finish({ pending: 0, rejected: 0, delivered: 1, awaitingDestination: 0, lastError: null })
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Retry retained contacts' })).toBeNull())
  expect(screen.getByText(/contact delivery caught up/)).toBeTruthy()
  expect(retryShackCqRejectedContacts).toHaveBeenCalledOnce()
})
