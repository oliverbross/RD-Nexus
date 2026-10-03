// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { openPanelWindow } from './api'
import { dismissToast, subscribeToasts, type Toast } from './toast'

describe('openPanelWindow', () => {
  let seen: Toast[] = []
  let unsubscribe = () => {}

  beforeEach(() => {
    seen = []
    unsubscribe = subscribeToasts((next) => { seen = next })
  })

  afterEach(() => {
    unsubscribe()
    for (const toast of seen) dismissToast(toast.id)
    delete window.__TAURI_INTERNALS__
  })

  it('surfaces a native window failure instead of dropping the rejection', async () => {
    window.__TAURI_INTERNALS__ = {
      invoke: vi.fn(async () => { throw new Error('window creation refused') }),
    }

    await expect(openPanelWindow('needed')).resolves.toBeUndefined()
    expect(seen[seen.length - 1]?.kind).toBe('error')
    expect(seen[seen.length - 1]?.message).toContain('window creation refused')
  })
})
