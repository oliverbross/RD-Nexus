export interface InitialSnapshotLoadOptions<T> {
  load: () => Promise<T>
  onValue: (value: T) => void
  onError: (error: unknown) => void
  onTimeout: () => void
  timeoutMs?: number
}

/** Start one cancellable boot read. A timeout is informative: a later successful read may recover. */
export function startInitialSnapshotLoad<T>({
  load,
  onValue,
  onError,
  onTimeout,
  timeoutMs = 12_000,
}: InitialSnapshotLoadOptions<T>): () => void {
  let active = true
  const timer = globalThis.setTimeout(() => {
    if (active) onTimeout()
  }, timeoutMs)

  void load().then(
    (value) => {
      if (!active) return
      active = false
      globalThis.clearTimeout(timer)
      onValue(value)
    },
    (error) => {
      if (!active) return
      active = false
      globalThis.clearTimeout(timer)
      onError(error)
    },
  )

  return () => {
    active = false
    globalThis.clearTimeout(timer)
  }
}
