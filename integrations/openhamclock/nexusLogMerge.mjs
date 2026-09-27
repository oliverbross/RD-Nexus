/** Reconcile a durable set of previously mirrored Nexus ids with a fresh snapshot.
 * Native OpenHamClock rows are preserved; absent Nexus rows are removed; a row
 * bearing the same explicit OpenHamClock binding id is replaced, never doubled. */
export function reconcileNexusLog(current, previousIds, incoming) {
  const oldIds = new Set(previousIds || [])
  const nextRows = Array.isArray(incoming) ? incoming.map((record) => ({ ...record })) : []
  const nextIds = new Set(nextRows.map((record) => record.id).filter(Boolean))
  const merged = new Map()
  for (const record of Array.isArray(current) ? current : []) {
    if (!oldIds.has(record.id) || nextIds.has(record.id)) merged.set(record.id, record)
  }
  for (const record of nextRows) merged.set(record.id, record)
  return { records: [...merged.values()], ids: [...nextIds] }
}
