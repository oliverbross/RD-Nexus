// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { DxChaserDxccMatrix, DxChaserSettings, DxChaserSnapshot } from '../api'
import { DxChaserView } from './DxChaserView'

const matrix: DxChaserDxccMatrix = {
  bands: ['160m', '80m', '40m', '30m', '20m', '17m', '15m', '12m', '10m'],
  filters: { totalEntities: 3, workedEntities: 1, unworkedEntities: 2, spottedUnworkedEntities: 1 },
  generatedAt: 1_700_000_000,
  entities: [
    { name: 'Bouvet Island', prefix: '3Y/B', rank: 2, worked: Array(9).fill(false), activeSpots: [{ call: '3Y0J', band: '20m', freqMhz: 14.074, source: 'DX cluster', spotter: 'W1AW', mode: 'FT8', receivedUnix: 1_699_999_940, ageSeconds: 60 }], workedAny: false, workedAll: false, hasActiveSpot: true, searchText: '3y/b bouvet island 2' },
    { name: 'Canada', prefix: 'VE', rank: null, worked: Array(9).fill(false), activeSpots: [], workedAny: false, workedAll: false, hasActiveSpot: false, searchText: 've canada' },
    { name: 'United States', prefix: 'K', rank: null, worked: Array(9).fill(true), activeSpots: [], workedAny: true, workedAll: true, hasActiveSpot: false, searchText: 'k united states' },
  ],
}

const { getDxChaserDxccMatrix, getFeedHealth } = vi.hoisted(() => ({
  getDxChaserDxccMatrix: vi.fn(),
  getFeedHealth: vi.fn(),
}))
vi.mock('../api', async importOriginal => {
  const actual = await importOriginal<typeof import('../api')>()
  return {
    ...actual,
    getDxChaserDxccMatrix, getFeedHealth,
    getDxChaser: vi.fn(), configureDxChaser: vi.fn(), resetDxChaserStatistics: vi.fn(),
    setDxChaserArmed: vi.fn(), setDxChaserTwilio: vi.fn(), stopDxChaser: vi.fn(),
    testDxChaserTwilio: vi.fn(), clearDxChaserTwilio: vi.fn(),
  }
})

const settings: DxChaserSettings = {
  enabled: true, enabledBands: ['20m'], useRbn: true, usePskReporter: true,
  useDxCluster: true, usSpottersOnly: true, scarceMode: false, ownCountryFilter: false,
  usGridOnly: true, missedCycles: 4, scarceMissedCycles: 8, atnoHardMaxCycles: 20,
  scarceThreshold: 10, cooldownSecs: 60, atnoCooldownSecs: 120, decodeTtlSecs: 60, spotTtlSecs: 300,
  crossBandAlertCooldownSecs: 60, localVerificationSecs: 30, nonstandardQsyCooldownSecs: 60,
  nonstandardThresholdHz: 1000, minExternalSpotters: 2, slotMinHz: 300, slotMaxHz: 2700,
  slotWidthHz: 60, txOffsetDeltaHz: 300, scoreNewDxcc: 100, scoreNewBand: 50,
  scoreNewGrid: 25, scoreCq: 10, scoreSnrPerDb: 1, scoreRepeat: 2, scoreSpotter: 3,
  scoreCallingMe: 200, minSnr: -20, atnoChasePolicy: 'both', autoBand: false,
  hunterMode: false, minDwellSecs: 120, hysteresis: 0.25, quietSpotThreshold: 3,
  pounceCooldownSecs: 60, deferredPounceExpireSecs: 120, maxDeferredPounces: 10,
  timeBasedExclusions: true, utcOffsetHours: 0, decodeHistoryRetentionCycles: 8,
  autoQsyNonstandard: false, smsMode: 'disabled', smsMinIntervalSecs: 60,
  quietHoursStart: null, quietHoursEnd: null,
}
const snapshot: DxChaserSnapshot = {
  upstreamRevision: 'd960ae22de78940c', state: 'listening', automationArmed: false,
  lastError: null, settings, target: null, candidates: [], bands: [], bandHistory: [],
  decodeHistory: [], deferredPounces: 0, targetCyclesRemaining: null,
  localVerificationRemainingSecs: null, sources: [], activeSpotPairs: 0, sessionStatistics: { startedAt: 1_699_999_000,
    evaluated: 0, selected: 0, completed: 0, abandoned: 0, bandChanges: 0,
    nonstandardQsys: 0, smsQueued: 0, newDxccWorked: 0, newBandSlots: 0, newGrids: 0 },
  allTimeStatistics: { startedAt: 1_600_000_000, evaluated: 40, selected: 12,
    completed: 9, abandoned: 3, bandChanges: 5, nonstandardQsys: 2, smsQueued: 4,
    newDxccWorked: 3, newBandSlots: 6, newGrids: 7 },
  log: [], txGuard: 'disarmed',
}

beforeEach(() => {
  getFeedHealth.mockResolvedValue({
    cluster: { enabled: false, state: 'off', lastEventSecs: null },
    pskr: { enabled: false, state: 'off', lastEventSecs: null },
    phoneCluster: { enabled: false, state: 'off', lastEventSecs: null },
    phoneClusterHost: null, phoneSpotsSeen: 0,
  })
})
afterEach(() => { cleanup(); vi.useRealTimers(); getDxChaserDxccMatrix.mockClear(); getFeedHealth.mockReset() })

describe('DX Chaser DXCC browser', () => {
  it('renders the nine-band matrix, filters entities, and exposes spots without radio actions', async () => {
    getDxChaserDxccMatrix.mockResolvedValue(matrix)
    const { container } = render(<DxChaserView initialModel={snapshot} />)
    expect(container.querySelectorAll('.dx-chaser-scroll')).toHaveLength(1)

    fireEvent.click(screen.getByRole('tab', { name: 'DXCC matrix' }))
    await screen.findByRole('table', { name: /DXCC entities by amateur-radio band/i })
    expect(getDxChaserDxccMatrix).toHaveBeenCalledTimes(1)
    const spot = screen.getByText('3Y0J').closest('li')
    expect(spot?.textContent).toContain('14.074 MHz · FT8')
    expect(screen.getAllByRole('columnheader').map(node => node.textContent)).toEqual(expect.arrayContaining(matrix.bands))

    fireEvent.click(screen.getByRole('button', { name: /Spotted and needed 1/i }))
    expect(screen.getByText('Bouvet Island')).toBeTruthy()
    expect(screen.queryByText('Canada')).toBeNull()
    expect(screen.queryByText('United States')).toBeNull()
    expect(screen.getAllByRole('button').some(button => /^(QSY|Call)/i.test(button.textContent ?? ''))).toBe(false)

    fireEvent.click(screen.getByRole('button', { name: /All 3/i }))
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search entities' }), { target: { value: 'Canada' } })
    expect(screen.getByText('Canada')).toBeTruthy()
    expect(screen.queryByText('Bouvet Island')).toBeNull()
  }, 15_000)

  it('exposes scarce mode as an explicit saved setting', async () => {
    render(<DxChaserView initialModel={snapshot} />)
    fireEvent.click(screen.getByRole('tab', { name: 'Settings' }))
    const control = screen.getByRole('checkbox', { name: /Scarce DX mode/i }) as HTMLInputElement
    expect(control.checked).toBe(false)
    fireEvent.click(control)
    await waitFor(() => expect(control.checked).toBe(true))
  })

  it('shows authoritative feed health, source controls, SMS interval, and split statistics', async () => {
    getFeedHealth.mockResolvedValue({
      cluster: { enabled: true, state: 'live', lastEventSecs: 4 },
      pskr: { enabled: true, state: 'connected', lastEventSecs: null },
      phoneCluster: { enabled: true, state: 'live', lastEventSecs: 9 },
      phoneClusterHost: 'dxc.example.net', phoneSpotsSeen: 12,
    })
    render(<DxChaserView initialModel={snapshot} />)
    expect(await screen.findByText('Cluster / RBN aggregate')).toBeTruthy()
    expect(screen.getByText(/dxc\.example\.net · 12 phone spots/)).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: 'Settings' }))
    expect((screen.getByRole('checkbox', { name: /Use Reverse Beacon Network/i }) as HTMLInputElement).checked).toBe(true)
    expect((screen.getByRole('checkbox', { name: /US spotters only/i }) as HTMLInputElement).checked).toBe(true)
    expect(screen.getByRole('checkbox', { name: '6m' })).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: '60m' })).toBeNull()
    const interval = screen.getByRole('spinbutton', { name: /Minimum SMS interval/i }) as HTMLInputElement
    expect(interval.value).toBe('60')
    expect(interval.min).toBe('10')
    expect(interval.max).toBe('3600')

    fireEvent.click(screen.getByRole('tab', { name: 'Statistics' }))
    expect(screen.getByRole('heading', { name: 'This session' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /All time/ })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Reset session statistics' })).toBeTruthy()
    expect(screen.getByText('9')).toBeTruthy()
  }, 15_000)

  it('refreshes the visible DXCC matrix every 30 seconds and stops after leaving', async () => {
    vi.useFakeTimers()
    getDxChaserDxccMatrix.mockResolvedValue(matrix)
    render(<DxChaserView initialModel={snapshot} />)
    fireEvent.click(screen.getByRole('tab', { name: 'DXCC matrix' }))
    await act(async () => { await Promise.resolve() })
    expect(getDxChaserDxccMatrix).toHaveBeenCalledTimes(1)
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
    expect(getDxChaserDxccMatrix).toHaveBeenCalledTimes(2)
    fireEvent.click(screen.getByRole('tab', { name: 'Settings' }))
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
    expect(getDxChaserDxccMatrix).toHaveBeenCalledTimes(2)
  })
})
