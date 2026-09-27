// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { NeuralDxAction } from '../api'

const api = vi.hoisted(() => ({
  getNeuralDxActions: vi.fn(async (): Promise<NeuralDxAction[]> => []),
  getNeuralDxStatus: vi.fn(async () => ({ phase: 'ready', url: 'http://127.0.0.1:9000', version: '12.7', detail: null })),
  restartNeuralDx: vi.fn(async () => undefined),
  syncNeuralDx: vi.fn(async (_payload: Record<string, unknown>) => undefined),
  executeNeuralDxAction: vi.fn(),
  ackNeuralDxActions: vi.fn(),
  resolveNeuralDxAction: vi.fn(),
}))

vi.mock('../api', () => api)
import { NeuralDxView, scoreHuntTargets, toWatcherSpot } from './NeuralDxView'
import { FEATURES, isHostedRemoteFeature } from '../features/registry'
import type { NeedAlert, SpotRow } from '../types'

const spot = (overrides: Partial<SpotRow> = {}): SpotRow => ({
  call: 'VK9DX',
  entity: 'Norfolk Island',
  zone: 32,
  grid: 'RG30',
  band: '20m',
  freqMhz: 14.195,
  mode: 'Phone',
  submode: null,
  spotter: 'K1ABC',
  corroborators: [],
  ageSecs: 30,
  comment: '',
  licensed: true,
  ...overrides,
})

describe('NeuralDxView', () => {
  beforeEach(() => {
    for (const mock of Object.values(api)) mock.mockClear()
  })

  it('is registered immediately below Spots in the main menu', () => {
    const spotsIndex = FEATURES.findIndex((feature) => feature.id === 'spots')
    expect(FEATURES[spotsIndex + 1]?.id).toBe('neuralDx')
  })

  it('does not advertise the desktop sidecar in hosted Remote', () => {
    expect(isHostedRemoteFeature('neuralDx')).toBe(false)
    expect(isHostedRemoteFeature('spots')).toBe(true)
  })

  it('gives same-second production spots distinct stable identities', () => {
    vi.spyOn(Date, 'now').mockReturnValue(1_700_000_031_100)
    const first = toWatcherSpot(spot({ spotter: 'K1AAA', freqMhz: 14.195, receivedUnix: 1_700_000_001 }), undefined, 'JN23')
    const second = toWatcherSpot(spot({ spotter: 'K2BBB', freqMhz: 14.196, receivedUnix: 1_700_000_001 }), undefined, 'JN23')
    expect(first.timestamp).toBe(second.timestamp)
    expect(first.spot_id).not.toBe(second.spot_id)
    expect(first.spot_id).toContain('K1AAA')
    expect(toWatcherSpot(spot({ spotter: 'K1AAA', freqMhz: 14.195, receivedUnix: 1_700_000_001 }), undefined, 'JN23').spot_id)
      .toBe(first.spot_id)
    vi.restoreAllMocks()
  })

  it('ranks an unworked needed long-path target above an ordinary worked spot', () => {
    const needed: NeedAlert = {
      call: 'VK9DX', entity: 'Norfolk Island', band: '20m', zone: 32,
      tags: ['NewEntity'], priority: 9, headline: 'New entity', mode: 'Phone', freqMhz: 14.195,
    }
    const ranked = scoreHuntTargets(
      [spot(), spot({ call: 'W1AW', entity: 'United States', grid: 'FN31', workedAgoSecs: 60 })],
      [needed],
      'JN23',
    )
    expect(ranked[0].spot.call).toBe('VK9DX')
    expect(ranked[0].need?.tags).toContain('NewEntity')
  })

  it('renders one bounded full-view scroll owner and routes shared-service tabs', () => {
    const navigate = vi.fn()
    const { container } = render(
      <NeuralDxView
        spots={[]}
        needs={[]}
        myGrid="JN23"
        logTick={0}
        selectedCall={null}
        onSelect={vi.fn()}
        onWork={vi.fn()}
        onNavigate={navigate}
      />,
    )
    expect(container.querySelectorAll('.neural-dx-view')).toHaveLength(1)
    expect(screen.getByText('0 live spots')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Satellites' }))
    fireEvent.click(screen.getByRole('button', { name: 'Open satellite console' }))
    expect(navigate).toHaveBeenCalledWith('sats')
  })

  it('sends only spots and lets native code supply station and log truth', async () => {
    const props = {
      needs: [], myGrid: 'JN23', logTick: 7, selectedCall: null,
      onSelect: vi.fn(), onWork: vi.fn(), onNavigate: vi.fn(),
    }
    const view = render(<NeuralDxView {...props} spots={[spot()]} />)
    await waitFor(() => expect(api.syncNeuralDx).toHaveBeenCalled())
    view.rerender(<NeuralDxView {...props} spots={[spot(), spot({ call: 'W1AW' })]} />)
    await waitFor(() => expect(api.syncNeuralDx).toHaveBeenCalled())
    for (const [payload] of api.syncNeuralDx.mock.calls) {
      expect(Object.keys(payload)).toEqual(['spots'])
    }
  })

  it('stops automatic replay and requires an operator decision for delivery-unknown', async () => {
    const action = { id: '0123456789abcdef0123456789abcdef', type: 'postSpot' as const,
      freqMhz: 14.195, call: 'VK9DX', comment: 'UP 5' }
    api.getNeuralDxActions.mockResolvedValueOnce([action])
    api.executeNeuralDxAction.mockResolvedValueOnce({ id: action.id, state: 'deliveryUnknown', detail: null })
    api.resolveNeuralDxAction.mockResolvedValueOnce({ id: action.id, state: 'delivered', detail: null })
    render(<NeuralDxView spots={[]} needs={[]} myGrid="JN23" logTick={0}
      selectedCall={null} onSelect={vi.fn()} onWork={vi.fn()} onNavigate={vi.fn()} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Mark delivered' }))
    await waitFor(() => expect(api.resolveNeuralDxAction).toHaveBeenCalledWith(action.id, 'delivered'))
    expect(api.ackNeuralDxActions).toHaveBeenCalledWith([action.id])
    expect(api.executeNeuralDxAction).toHaveBeenCalledTimes(1)
  })
})
