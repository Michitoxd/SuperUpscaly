import { atom } from 'jotai'
import type { ErrorCode } from '@superupscaly/shared'

export type RunPhase = 'idle' | 'running' | 'paused' | 'finished'

export const runPhaseAtom = atom<RunPhase>('idle')

/** true mientras haya un lote activo (en curso o en pausa). */
export const isBusyAtom = atom((get) => get(runPhaseAtom) === 'running' || get(runPhaseAtom) === 'paused')

export interface RunSummary {
  total: number
  done: number
  degraded: number
  failed: number
  totalMs: number
  avgMs: number
  cancelled: boolean
}

export const runSummaryAtom = atom<RunSummary | null>(null)

/** Controla la visibilidad del dialogo de resumen final. */
export const summaryOpenAtom = atom(false)

export interface LastError {
  code: ErrorCode
  detail: string
}

export const lastErrorAtom = atom<LastError | null>(null)
