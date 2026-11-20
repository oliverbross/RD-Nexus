import { useCallback, useEffect, useState } from 'react'
import type { View } from './features/registry'
import { getShackCqProductMode, isTauri, setShackCqProductMode } from './api'

export type ProductMode = 'basic' | 'digi'

const STORAGE_KEY = 'shackcq.productMode'
const BASIC_VIEWS = new Set<View>(['connect', 'needed', 'settings'])

export function normalizeProductMode(value: string | null | undefined): ProductMode {
  return value === 'digi' ? 'digi' : 'basic'
}

export function productModeAllowsView(mode: ProductMode, view: View): boolean {
  return mode === 'digi' || BASIC_VIEWS.has(view)
}

export function loadProductMode(storage: Pick<Storage, 'getItem'> | undefined = globalThis.localStorage): ProductMode {
  try {
    return normalizeProductMode(storage?.getItem(STORAGE_KEY))
  } catch {
    return 'basic'
  }
}

export function useProductMode(remote = false): readonly [ProductMode, (mode: ProductMode) => Promise<boolean>, boolean] {
  const native = !remote && isTauri()
  // A webview preference cannot silently enable native audio. Native startup is authoritative.
  const [mode, setModeState] = useState<ProductMode>(() => native ? 'basic' : loadProductMode())
  const [isolated, setIsolated] = useState(false)
  const [ready, setReady] = useState(!native)
  useEffect(() => {
    if (!native) return
    let cancelled = false
    void getShackCqProductMode().then(status => {
      if (cancelled) return
      setModeState(status.mode)
      setIsolated(status.isolated)
      setReady(true)
    }).catch(() => { /* Keep Basic visible; switching reports the unavailable native state. */ })
    return () => { cancelled = true }
  }, [native])
  const setMode = useCallback(async (next: ProductMode) => {
    if (!ready) throw new Error('Desktop mode is still loading. Try again shortly.')
    if (native) {
      const status = await setShackCqProductMode(next)
      // Keep the current cockpit until the old native owner has shut down. The new
      // process resolves the saved choice before opening its receive audio backend.
      if (status.restartRequired) return false
      setModeState(status.mode)
      return true
    }
    setModeState(next)
    try {
      localStorage.setItem(STORAGE_KEY, next)
    } catch {
      // Storage may be blocked; the selected mode still applies for this run.
    }
    return true
  }, [native, ready])
  return [mode, setMode, native && !isolated] as const
}
