// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getShackCqProductMode, isTauri, setShackCqProductMode } from './api'
import { useProductMode } from './productMode'

vi.mock('./api', () => ({
  isTauri: vi.fn(), getShackCqProductMode: vi.fn(), setShackCqProductMode: vi.fn(),
}))
beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  vi.mocked(isTauri).mockReturnValue(true)
})
afterEach(cleanup)

describe('native Basic/Digi lifecycle', () => {
  it('uses native startup mode even when a webview retained the opposite selection', async () => {
    localStorage.setItem('shackcq.productMode', 'digi')
    vi.mocked(getShackCqProductMode).mockResolvedValue({ mode: 'basic', isolated: false, restartRequired: false })
    const { result } = renderHook(() => useProductMode())
    await waitFor(() => expect(getShackCqProductMode).toHaveBeenCalledOnce())
    expect(result.current[0]).toBe('basic')
    expect(result.current[2]).toBe(true)
    expect(setShackCqProductMode).not.toHaveBeenCalled()
  })

  it('requests native persistence/restart without presenting Digi as active in the old process', async () => {
    vi.mocked(getShackCqProductMode).mockResolvedValue({ mode: 'basic', isolated: false, restartRequired: false })
    vi.mocked(setShackCqProductMode).mockResolvedValue({ mode: 'digi', isolated: false, restartRequired: true })
    const { result } = renderHook(() => useProductMode())
    await act(async () => {})
    await act(async () => { expect(await result.current[1]('digi')).toBe(false) })
    expect(setShackCqProductMode).toHaveBeenCalledWith('digi')
    expect(result.current[0]).toBe('basic')
    expect(localStorage.getItem('shackcq.productMode')).toBeNull()
  })

  it('allows fixture cockpit switching while the admitted manifest retains audio ownership', async () => {
    vi.mocked(getShackCqProductMode).mockResolvedValue({ mode: 'basic', isolated: true, restartRequired: false })
    vi.mocked(setShackCqProductMode).mockResolvedValue({ mode: 'digi', isolated: true, restartRequired: false })
    const { result } = renderHook(() => useProductMode())
    await waitFor(() => expect(result.current[2]).toBe(false))
    await act(async () => { expect(await result.current[1]('digi')).toBe(true) })
    expect(result.current[0]).toBe('digi')
  })

  it('keeps the active mode on native persistence failure', async () => {
    vi.mocked(getShackCqProductMode).mockResolvedValue({ mode: 'digi', isolated: false, restartRequired: false })
    vi.mocked(setShackCqProductMode).mockRejectedValue(new Error('storage unavailable'))
    const { result } = renderHook(() => useProductMode())
    await waitFor(() => expect(result.current[0]).toBe('digi'))
    await act(async () => { await expect(result.current[1]('basic')).rejects.toThrow('storage unavailable') })
    expect(result.current[0]).toBe('digi')
  })

  it('keeps remote presentation changes out of the native lifecycle', async () => {
    const { result } = renderHook(() => useProductMode(true))
    await act(async () => { expect(await result.current[1]('digi')).toBe(true) })
    expect(result.current[0]).toBe('digi')
    expect(getShackCqProductMode).not.toHaveBeenCalled()
    expect(setShackCqProductMode).not.toHaveBeenCalled()
  })
})
