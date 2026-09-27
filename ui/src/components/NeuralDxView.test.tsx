// @vitest-environment jsdom

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { NeuralDxView, scoreHuntTargets } from './NeuralDxView'
import { FEATURES } from '../features/registry'
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
  it('is registered immediately below Spots in the main menu', () => {
    const spotsIndex = FEATURES.findIndex((feature) => feature.id === 'spots')
    expect(FEATURES[spotsIndex + 1]?.id).toBe('neuralDx')
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
        myCall="F1TEST"
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
})
