import { describe, expect, it } from 'vitest'
import { loadProductMode, normalizeProductMode, productModeAllowsView } from './productMode'

describe('ShackCQ product mode', () => {
  it('defaults fail-closed to Basic', () => {
    expect(normalizeProductMode(null)).toBe('basic')
    expect(normalizeProductMode('unknown')).toBe('basic')
    expect(loadProductMode({ getItem: () => null })).toBe('basic')
  })

  it('keeps Digi explicit and persistent', () => {
    expect(normalizeProductMode('digi')).toBe('digi')
    expect(loadProductMode({ getItem: () => 'digi' })).toBe('digi')
  })

  it('limits Basic to radio, rotator and configuration surfaces', () => {
    expect(productModeAllowsView('basic', 'connect')).toBe(true)
    expect(productModeAllowsView('basic', 'needed')).toBe(true)
    expect(productModeAllowsView('basic', 'settings')).toBe(true)
    expect(productModeAllowsView('basic', 'operate')).toBe(false)
    expect(productModeAllowsView('basic', 'sstv')).toBe(false)
    expect(productModeAllowsView('digi', 'sstv')).toBe(true)
  })
})
