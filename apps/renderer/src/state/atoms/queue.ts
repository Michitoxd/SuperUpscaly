import { atom } from 'jotai'
import type { QueueItem } from '@superupscaly/shared'

/** Imagenes en la cola, en el orden en que se procesaran. */
export const queueAtom = atom<QueueItem[]>([])

export interface QueueStats {
  total: number
  pending: number
  running: number
  done: number
  degraded: number
  failed: number
  totalBytes: number
  /** Progreso agregado 0..1, ponderado por item (no por bytes). */
  overallProgress: number
}

export const queueStatsAtom = atom<QueueStats>((get) => {
  const items = get(queueAtom)
  let pending = 0
  let running = 0
  let done = 0
  let degraded = 0
  let failed = 0
  let totalBytes = 0
  let progressSum = 0

  for (const item of items) {
    totalBytes += item.sizeBytes
    progressSum += item.progress
    switch (item.status) {
      case 'pending':
        pending += 1
        break
      case 'running':
        running += 1
        break
      case 'done':
        done += 1
        break
      case 'degraded':
        degraded += 1
        break
      case 'failed':
        failed += 1
        break
      default:
        break
    }
  }

  return {
    total: items.length,
    pending,
    running,
    done,
    degraded,
    failed,
    totalBytes,
    overallProgress: items.length > 0 ? progressSum / items.length : 0,
  }
})

export const hasQueueItemsAtom = atom((get) => get(queueAtom).length > 0)

/** Aplica un parche a un item concreto sin re-renderizar la lista completa. */
export const patchQueueItemAtom = atom(
  null,
  (get, set, id: string, patch: Partial<QueueItem>): void => {
    const items = get(queueAtom)
    const index = items.findIndex((item) => item.id === id)
    if (index < 0) return
    const current = items[index]
    if (!current) return
    const next = [...items]
    next[index] = { ...current, ...patch }
    set(queueAtom, next)
  },
)

export const appendToQueueAtom = atom(null, (get, set, incoming: QueueItem[]): void => {
  set(queueAtom, [...get(queueAtom), ...incoming])
})

export const removeFromQueueAtom = atom(null, (get, set, id: string): void => {
  set(
    queueAtom,
    get(queueAtom).filter((item) => item.id !== id),
  )
})

export const clearQueueAtom = atom(null, (_get, set): void => {
  set(queueAtom, [])
})

/** Devuelve un item al estado inicial de ejecucion. */
export function normalizeForRun(item: QueueItem): QueueItem {
  const next: QueueItem = {
    id: item.id,
    path: item.path,
    name: item.name,
    dir: item.dir,
    sizeBytes: item.sizeBytes,
    status: 'pending',
    progress: 0,
  }
  return next
}

/** Devuelve todos los items a `pending` para poder relanzar el lote. */
export const resetQueueAtom = atom(null, (get, set): void => {
  set(queueAtom, get(queueAtom).map(normalizeForRun))
})
