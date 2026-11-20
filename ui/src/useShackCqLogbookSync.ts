import { useEffect, useState } from 'react'
import {
  getShackCqPairing,
  syncShackCqSelectedLogbook,
  type ShackCqLogbookSync,
} from './api'

const SYNC_INTERVAL_MS = 5 * 60_000

export type ShackCqSyncState =
  | { phase: 'inactive' | 'unpaired' | 'syncing' }
  | { phase: 'ready'; result: ShackCqLogbookSync; syncedUtc: string }
  | { phase: 'error'; message: string }

export function useShackCqLogbookSync(enabled: boolean): ShackCqSyncState {
  const [state, setState] = useState<ShackCqSyncState>({ phase: enabled ? 'unpaired' : 'inactive' })
  useEffect(() => {
    if (!enabled) {
      setState({ phase: 'inactive' })
      return
    }
    let alive = true
    let running = false
    let rerunRequested = false
    const sync = async () => {
      if (running) {
        rerunRequested = true
        return
      }
      running = true
      if (alive) setState({ phase: 'syncing' })
      try {
        const pairing = await getShackCqPairing()
        if (!pairing) {
          if (alive) setState({ phase: 'unpaired' })
          return
        }
        const result = await syncShackCqSelectedLogbook(pairing.auth)
        if (alive) setState({ phase: 'ready', result, syncedUtc: new Date().toISOString() })
      } catch (error) {
        if (alive) setState({ phase: 'error', message: error instanceof Error ? error.message : String(error) })
      } finally {
        running = false
        if (alive && rerunRequested) {
          rerunRequested = false
          void sync()
        }
      }
    }
    void sync()
    const timer = window.setInterval(() => void sync(), SYNC_INTERVAL_MS)
    const pairingChanged = () => void sync()
    const contactDelivered = () => void sync()
    window.addEventListener('shackcq-pairing-changed', pairingChanged)
    window.addEventListener('shackcq-contact-outbox-delivered', contactDelivered)
    return () => {
      alive = false
      window.clearInterval(timer)
      window.removeEventListener('shackcq-pairing-changed', pairingChanged)
      window.removeEventListener('shackcq-contact-outbox-delivered', contactDelivered)
    }
  }, [enabled])
  return state
}
