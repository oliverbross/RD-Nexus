import { afterEach, describe, expect, it, vi } from 'vitest'
import { startInitialSnapshotLoad } from './initialSnapshotLoad'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => {
    resolve = yes
    reject = no
  })
  return { promise, resolve, reject }
}

afterEach(() => {
  vi.useRealTimers()
})

describe('startInitialSnapshotLoad', () => {
  it('reports a rejection and cancels its timeout', async () => {
    vi.useFakeTimers()
    const onError = vi.fn()
    const onTimeout = vi.fn()
    startInitialSnapshotLoad({
      load: async () => { throw new Error('station unavailable') },
      onValue: vi.fn(),
      onError,
      onTimeout,
    })
    await Promise.resolve()
    expect(onError.mock.calls[0]?.[0]).toEqual(new Error('station unavailable'))
    await vi.advanceTimersByTimeAsync(12_000)
    expect(onTimeout).not.toHaveBeenCalled()
  })

  it('shows a timeout but still accepts a later successful read', async () => {
    vi.useFakeTimers()
    const pending = deferred<string>()
    const onValue = vi.fn()
    const onTimeout = vi.fn()
    startInitialSnapshotLoad({
      load: () => pending.promise,
      onValue,
      onError: vi.fn(),
      onTimeout,
    })
    await vi.advanceTimersByTimeAsync(12_000)
    expect(onTimeout).toHaveBeenCalledOnce()
    pending.resolve('ready')
    await Promise.resolve()
    expect(onValue).toHaveBeenCalledWith('ready')
  })

  it('suppresses a cancelled stale attempt while a retry succeeds', async () => {
    vi.useFakeTimers()
    const stale = deferred<string>()
    const current = deferred<string>()
    const onValue = vi.fn()
    const staleTimeout = vi.fn()
    const cancelStale = startInitialSnapshotLoad({
      load: () => stale.promise,
      onValue,
      onError: vi.fn(),
      onTimeout: staleTimeout,
    })
    cancelStale()
    startInitialSnapshotLoad({
      load: () => current.promise,
      onValue,
      onError: vi.fn(),
      onTimeout: vi.fn(),
    })
    stale.resolve('stale')
    current.resolve('current')
    await Promise.resolve()
    expect(onValue).toHaveBeenCalledTimes(1)
    expect(onValue).toHaveBeenCalledWith('current')
    await vi.advanceTimersByTimeAsync(12_000)
    expect(staleTimeout).not.toHaveBeenCalled()
  })
})
