// @vitest-environment jsdom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppSnapshot } from '../types'

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  restart: vi.fn(),
}))

vi.mock('../api', () => ({
  openHamClockStatus: mocks.status,
  restartOpenHamClock: mocks.restart,
  askLog: vi.fn().mockResolvedValue({ rows: [], editKeys: [], total: 0 }),
  getAllSpots: vi.fn().mockResolvedValue([]),
  readRotator: vi.fn().mockResolvedValue(null),
  deleteQsoById: vi.fn(),
  editQsoById: vi.fn(),
  logQso: vi.fn(),
  pointRotator: vi.fn(),
  postSpot: vi.fn(),
  purgeLog: vi.fn(),
  setFrequency: vi.fn(),
  setPtt: vi.fn(),
  stopRotator: vi.fn(),
}))

import { HamClockView } from './HamClockView'

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

  beforeEach(() => {
    ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
    vi.useFakeTimers()
    host = document.createElement('div')
    document.body.append(host)
    root = createRoot(host)
    mocks.restart.mockReset().mockResolvedValue(undefined)
  })

  afterEach(async () => {
    await act(async () => root.unmount())
    host.remove()
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
})
