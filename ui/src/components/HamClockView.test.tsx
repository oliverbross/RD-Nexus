// @vitest-environment jsdom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppSnapshot } from '../types'

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  restart: vi.fn(),
  confirm: vi.fn(),
  purge: vi.fn(),
  ptt: vi.fn(),
  askLog: vi.fn(),
  editQso: vi.fn(),
  deleteQso: vi.fn(),
  logDelta: vi.fn(),
}))

vi.mock('../confirm', () => ({ confirmDialog: mocks.confirm }))

vi.mock('../api', () => ({
  openHamClockStatus: mocks.status,
  openHamClockLogDelta: mocks.logDelta,
  restartOpenHamClock: mocks.restart,
  askLog: mocks.askLog,
  getAllSpots: vi.fn().mockResolvedValue([]),
  readRotator: vi.fn().mockResolvedValue(null),
  deleteQsoById: mocks.deleteQso,
  editQsoById: mocks.editQso,
  logQso: vi.fn(),
  pointRotator: vi.fn(),
  postSpot: vi.fn(),
  purgeLog: mocks.purge,
  exportLog: vi.fn().mockResolvedValue('<EOH>\n'),
  saveTextToDownloads: vi.fn().mockResolvedValue('/tmp/backup.adi'),
  setFrequency: vi.fn(),
  setOpenHamClockPtt: mocks.ptt,
  stopRotator: vi.fn(),
}))

import { HamClockView, chunkOpenHamLogbook, fromOpenHam, loadOpenHamLogbook, openHamBridgeMessageBytes, toOpenHam, LOGBOOK_CHUNK_BYTES, LOGBOOK_CHUNK_ROWS, MAX_LOGBOOK_PAGES } from './HamClockView'

const snap = {
  mycall: 'VK8AA',
  mygrid: 'PH57kp',
  mode: 'FT8',
  recentDecodes: [],
  radio: {
    dialMhz: 14.074,
    band: '20m',
    sideband: 'USB',
    transmitting: false,
    catOk: true,
  },
} as unknown as AppSnapshot

describe('HamClockView', () => {
  let host: HTMLDivElement
  let root: Root
  let frameWindow: Window
  let autoAck: boolean
  let pendingAcks: Array<() => void>

  beforeEach(() => {
    ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
    vi.useFakeTimers()
    host = document.createElement('div')
    document.body.append(host)
    root = createRoot(host)
    autoAck = true
    pendingAcks = []
    mocks.restart.mockReset().mockResolvedValue(undefined)
    mocks.confirm.mockReset().mockResolvedValue(false)
    mocks.purge.mockReset().mockResolvedValue(0)
    mocks.ptt.mockReset().mockResolvedValue(snap)
    mocks.askLog.mockReset().mockResolvedValue({ rows: [], editKeys: [], total: 0, offset: 0, revision: 1 })
    mocks.logDelta.mockReset().mockImplementation(async (revision: number) => ({ revision, reset: false, upserts: [], deleted: [] }))
    mocks.editQso.mockReset().mockResolvedValue({ kind: 'applied' })
    mocks.deleteQso.mockReset().mockResolvedValue({ kind: 'deleted' })
    const snapshotRevisions = new Map<number, number>()
    const postMessage = vi.fn((message: unknown, targetOrigin?: string) => {
      const envelope = message as { source?: string; type?: string; payload?: Record<string, unknown> }
      if (envelope.source !== 'nexus') return
      if (envelope.type === 'logbook:begin') {
        snapshotRevisions.set(Number(envelope.payload?.revision), Number(envelope.payload?.nativeRevision))
      }
      const messageRevision = envelope.type === 'logbook:end'
        ? Number(envelope.payload?.revision)
        : envelope.type === 'logbook:delta' ? Number(envelope.payload?.messageRevision) : 0
      if (!messageRevision) return
      const revision = envelope.type === 'logbook:end'
        ? snapshotRevisions.get(messageRevision) ?? 0
        : Number(envelope.payload?.revision)
      const acknowledge = () => window.dispatchEvent(new MessageEvent('message', {
        origin: targetOrigin,
        source: frameWindow,
        data: { source: 'openhamclock', type: 'logbook:commit', payload: { messageRevision, status: 'committed', revision } },
      }))
      if (autoAck) queueMicrotask(acknowledge)
      else pendingAcks.push(acknowledge)
    })
    frameWindow = { postMessage } as unknown as Window
    vi.spyOn(HTMLIFrameElement.prototype, 'contentWindow', 'get').mockReturnValue(frameWindow)
  })

  afterEach(async () => {
    await act(async () => root.unmount())
    host.remove()
    vi.restoreAllMocks()
    vi.useRealTimers()
    ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = false
  })

  it('loads the supervised loopback app as the Ham Clock screen', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    await act(async () => root.render(<HamClockView snap={snap} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    const frame = host.querySelector('iframe')
    expect(frame?.getAttribute('src')).toBe('http://127.0.0.1:39001/?nexus=1')
    expect(frame?.getAttribute('title')).toBeTruthy()
  })

  it('shows a truthful unavailable state and restarts the owned process', async () => {
    mocks.status.mockResolvedValue({ phase: 'unavailable', url: null, version: '26.8.0', detail: 'not staged' })
    await act(async () => root.render(<HamClockView snap={snap} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    const button = host.querySelector('button') as HTMLButtonElement
    expect(host.textContent).toContain('not staged')
    await act(async () => button.click())
    expect(mocks.restart).toHaveBeenCalledOnce()
  })

  it('preserves modeled confirmations and unknown ADIF fields through the Nexus seam', () => {
    const nexus = fromOpenHam({
      id: 'ohc-1', call: 'K1ABC', qso_date: '20260927', time_on: '123456', band: '20m', mode: 'FT8',
      lotw_qsl_rcvd: 'Y', eqsl_qsl_rcvd: 'Y', dxcc: '291', prop_mode: 'SAT', sat_name: 'AO-91',
      extras: { APP_CUSTOM: 'kept', MY_ANTENNA: 'DIPOLE' },
    })
    expect(nexus.confirmed).toBe(true)
    expect(nexus.awardConfirmed).toBe(true)
    expect(nexus.extra).toContainEqual(['APP_CUSTOM', 'kept'])
    const roundTrip = toOpenHam(nexus, 'edit-key')
    expect(roundTrip.lotw_qsl_rcvd).toBe('Y')
    expect(roundTrip.sat_name).toBe('AO-91')
    expect(roundTrip.extras).toMatchObject({ APP_CUSTOM: 'kept', MY_ANTENNA: 'DIPOLE', APP_OPENHAMCLOCK_ID: 'ohc-1' })
  })

  it('uses explicit OpenHamClock ids and bounded chunks without collapsing same-minute contacts', () => {
    const first = fromOpenHam({ id: 'ohc-a', call: 'K1ABC', qso_date: '20260927', time_on: '123401', band: '20m', mode: 'FT8' })
    const second = fromOpenHam({ id: 'ohc-b', call: 'K1ABC', qso_date: '20260927', time_on: '123459', band: '20m', mode: 'FT8' })
    expect(first.extra).toContainEqual(['APP_OPENHAMCLOCK_ID', 'ohc-a'])
    expect(second.extra).toContainEqual(['APP_OPENHAMCLOCK_ID', 'ohc-b'])
    const rows = Array.from({ length: 67_001 }, (_, index) => ({ id: `nexus-${index}`, call: `K${index}ABC` }))
    const chunks = chunkOpenHamLogbook(rows)
    expect(chunks.flat()).toHaveLength(rows.length)
    expect(chunks).toHaveLength(Math.ceil(rows.length / LOGBOOK_CHUNK_ROWS))
    expect(chunks.length + 2).toBeLessThan(MAX_LOGBOOK_PAGES + 3)
    expect(Math.max(...chunks.map((chunk) => chunk.length))).toBeLessThanOrEqual(LOGBOOK_CHUNK_ROWS)
    for (const [offset, chunk] of chunks.entries()) {
      expect(openHamBridgeMessageBytes({ source: 'nexus', type: 'logbook:chunk', payload: { revision: 1, offset, rows: chunk } })).toBeLessThanOrEqual(LOGBOOK_CHUNK_BYTES)
    }
    expect(() => chunkOpenHamLogbook([{ id: 'oversized', extras: { APP_BIG: 'x'.repeat(LOGBOOK_CHUNK_BYTES) } }])).toThrow('bridge safety limit')
  })

  it('bounds paging by total and rejects a no-progress page instead of looping', async () => {
    const total = 67_001
    mocks.askLog.mockImplementation(async (question: { offset: number; limit: number }) => {
      const count = Math.min(question.limit, total - question.offset)
      const rows = Array.from({ length: count }, (_, index) => ({
        id: `qso-${question.offset + index}`, call: `K${question.offset + index}ABC`, whenUnix: 1_795_000_000,
        band: '20m', freqMhz: 14.074, mode: 'FT8', extra: [],
      }))
      return { rows, editKeys: rows.map((row) => `edit-${row.id}`), total, offset: question.offset, revision: 42 }
    })
    const loaded = await loadOpenHamLogbook()
    expect(loaded.rows).toHaveLength(total)
    expect(mocks.askLog).toHaveBeenCalledTimes(Math.ceil(total / LOGBOOK_CHUNK_ROWS))

    mocks.askLog.mockReset()
    mocks.askLog
      .mockResolvedValueOnce({ rows: [{ ...snap, id: 'one' }], editKeys: ['edit-one'], total: 2, offset: 0, revision: 43 })
      .mockResolvedValueOnce({ rows: [], editKeys: [], total: 2, offset: 1, revision: 43 })
    await expect(loadOpenHamLogbook()).rejects.toThrow('made no progress')
    expect(mocks.askLog).toHaveBeenCalledTimes(2)

    mocks.askLog.mockReset().mockResolvedValue({ rows: [], editKeys: [], total: 1_000_001, offset: 0, revision: 44 })
    await expect(loadOpenHamLogbook()).rejects.toThrow('invalid or unsupported')
    expect(mocks.askLog).toHaveBeenCalledOnce()
  })

  it('rejects stale-frame destructive messages and fails closed when clear is cancelled', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    await act(async () => root.render(<HamClockView snap={snap} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    const frame = host.querySelector('iframe') as HTMLIFrameElement
    window.dispatchEvent(new MessageEvent('message', {
      origin: 'http://127.0.0.1:39001', source: window, data: { source: 'openhamclock', type: 'logbook:clear' },
    }))
    await act(async () => Promise.resolve())
    expect(mocks.confirm).not.toHaveBeenCalled()
    window.dispatchEvent(new MessageEvent('message', {
      origin: 'http://127.0.0.1:39001', source: frame.contentWindow, data: { source: 'openhamclock', type: 'logbook:clear' },
    }))
    await act(async () => Promise.resolve())
    expect(mocks.confirm).toHaveBeenCalledOnce()
    expect(mocks.purge).not.toHaveBeenCalled()
  })

  it('uses the canonical Nexus row id behind a durable OpenHamClock binding', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    const canonical = {
      id: 'nexus-1', call: 'K1ABC', whenUnix: 1_795_000_000, band: '20m', freqMhz: 14.074,
      mode: 'FT8', extra: [['APP_OPENHAMCLOCK_ID', 'ohc-1']],
    }
    mocks.askLog.mockResolvedValue({ rows: [canonical], editKeys: ['edit-1'], total: 1, offset: 0, revision: 45 })
    await act(async () => root.render(<HamClockView snap={snap} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    const frame = host.querySelector('iframe') as HTMLIFrameElement
    const posted = frameWindow.postMessage as unknown as ReturnType<typeof vi.fn>
    mocks.logDelta
      .mockResolvedValueOnce({ revision: 46, reset: false, upserts: [{ row: { ...canonical, call: 'K1XYZ' }, editKey: 'edit-2' }], deleted: [] })
      .mockResolvedValueOnce({ revision: 47, reset: false, upserts: [], deleted: ['nexus-1'] })
    const started = performance.now()
    window.dispatchEvent(new MessageEvent('message', {
      origin: 'http://127.0.0.1:39001', source: frame.contentWindow,
      data: { source: 'openhamclock', type: 'logbook:update', payload: { id: 'ohc-1', _nexusEditKey: 'edit-1', call: 'K1XYZ' } },
    }))
    await act(async () => Promise.resolve())
    expect(mocks.editQso.mock.calls[0]?.[0]).toEqual({ id: 'nexus-1', editKey: 'edit-1' })

    window.dispatchEvent(new MessageEvent('message', {
      origin: 'http://127.0.0.1:39001', source: frame.contentWindow,
      data: { source: 'openhamclock', type: 'logbook:remove', payload: { id: 'ohc-1' } },
    }))
    await act(async () => Promise.resolve())
    expect(mocks.deleteQso).toHaveBeenCalledWith({ id: 'nexus-1', editKey: 'edit-2' })
    expect(mocks.askLog).toHaveBeenCalledOnce()
    expect(mocks.logDelta).toHaveBeenCalledTimes(2)
    const deltaMessages = posted.mock.calls.map((call) => call[0] as { type?: string; payload?: unknown }).filter((message) => message.type === 'logbook:delta')
    expect(deltaMessages).toHaveLength(2)
    expect(Math.max(...deltaMessages.map((message) => new TextEncoder().encode(JSON.stringify(message)).length))).toBeLessThan(LOGBOOK_CHUNK_BYTES)
    expect(performance.now() - started).toBeLessThan(1_000)
  })

  it('serializes native deltas until the embedded mirror acknowledges each durable revision', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    mocks.askLog.mockResolvedValue({ rows: [], editKeys: [], total: 0, offset: 0, revision: 1 })
    await act(async () => root.render(<HamClockView snap={{ ...snap, logTick: 0 } as AppSnapshot} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())

    autoAck = false
    mocks.logDelta
      .mockResolvedValueOnce({ revision: 2, reset: false, upserts: [], deleted: [] })
      .mockResolvedValueOnce({ revision: 3, reset: false, upserts: [], deleted: [] })
    await act(async () => root.render(<HamClockView snap={{ ...snap, logTick: 1 } as AppSnapshot} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    expect(mocks.logDelta).toHaveBeenCalledTimes(1)

    await act(async () => root.render(<HamClockView snap={{ ...snap, logTick: 2 } as AppSnapshot} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    expect(mocks.logDelta).toHaveBeenCalledTimes(1)
    expect(pendingAcks).toHaveLength(1)

    await act(async () => { pendingAcks.shift()?.(); await Promise.resolve() })
    expect(mocks.logDelta).toHaveBeenCalledTimes(2)
    expect(mocks.logDelta.mock.calls[1]?.[0]).toBe(2)
    await act(async () => { pendingAcks.shift()?.(); await Promise.resolve() })
  })

  it('falls back to bounded snapshot chunks when an accumulated delta exceeds the WebView byte cap', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    mocks.askLog.mockResolvedValueOnce({ rows: [], editKeys: [], total: 0, offset: 0, revision: 1 })
    await act(async () => root.render(<HamClockView snap={{ ...snap, logTick: 0 } as AppSnapshot} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())

    const rows = Array.from({ length: 80 }, (_, index) => ({
      id: `nexus-${index}`, call: `K${index}ABC`, whenUnix: 1_795_000_000 + index,
      band: '20m', freqMhz: 14.074, mode: 'FT8', extra: [['APP_BIG', 'x'.repeat(9_000)]],
    }))
    mocks.logDelta.mockResolvedValueOnce({
      revision: 2, reset: false,
      upserts: rows.map((row, index) => ({ row, editKey: `edit-${index}` })),
      deleted: [],
    })
    mocks.askLog.mockResolvedValue({ rows, editKeys: rows.map((_, index) => `edit-${index}`), total: rows.length, offset: 0, revision: 2 })
    const posted = frameWindow.postMessage as unknown as ReturnType<typeof vi.fn>
    posted.mockClear()

    await act(async () => root.render(<HamClockView snap={{ ...snap, logTick: 1 } as AppSnapshot} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())

    const messages = posted.mock.calls.map((call) => call[0] as { type?: string; payload?: { rows?: Array<{ id?: string }> } })
    expect(messages.some((message) => message.type === 'logbook:delta')).toBe(false)
    expect(Math.max(...messages.map(openHamBridgeMessageBytes))).toBeLessThanOrEqual(LOGBOOK_CHUNK_BYTES)
    const transferred = messages.filter((message) => message.type === 'logbook:chunk').flatMap((message) => message.payload?.rows ?? [])
    expect(transferred.map((row) => row.id)).toEqual(rows.map((row) => row.id))
    expect(new Set(transferred.map((row) => row.id)).size).toBe(rows.length)
  })

  it('keeps an always-reachable Nexus un-key control over the embedded frame', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    const transmitting = { ...snap, radio: { ...snap.radio, transmitting: true } } as AppSnapshot
    await act(async () => root.render(<HamClockView snap={transmitting} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    const stop = [...host.querySelectorAll('button')].find((button) => button.textContent?.includes('STOP TX'))
    expect(stop).toBeTruthy()
    await act(async () => stop?.click())
    expect(mocks.ptt).toHaveBeenCalledWith(false)
  })

  it('surfaces a refused embedded PTT request and leaves Nexus as the only actuator', async () => {
    mocks.status.mockResolvedValue({ phase: 'ready', url: 'http://127.0.0.1:39001', version: '26.8.0', detail: null })
    mocks.ptt.mockRejectedValueOnce(new Error('Nexus transmit safety gate refused the OpenHamClock PTT request'))
    await act(async () => root.render(<HamClockView snap={snap} onSnap={vi.fn()} />))
    await act(async () => Promise.resolve())
    const frame = host.querySelector('iframe') as HTMLIFrameElement
    window.dispatchEvent(new MessageEvent('message', {
      origin: 'http://127.0.0.1:39001', source: frame.contentWindow,
      data: { source: 'openhamclock', type: 'rig:setPTT', payload: { ptt: true } },
    }))
    await act(async () => Promise.resolve())
    expect(mocks.ptt).toHaveBeenCalledWith(true)
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('transmit safety gate refused')
  })
})
