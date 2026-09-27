'use client'

import { useEffect, useRef, useState } from 'react'
import { useAtomValue } from 'jotai'
import { Button, Progress } from '@superupscaly/ui'
import type { ModelDownloadEvent } from '@superupscaly/shared'
import { hasQueueItemsAtom, queueStatsAtom } from '@/state/atoms/queue'
import { isBusyAtom, runPhaseAtom } from '@/state/atoms/run'
import { translatorAtom } from '@/state/atoms/settings'
import { downloadsAtom } from '@/state/atoms/models'
import { activeJobAtom } from '@/state/atoms/sidecar'
import { formatDuration, formatPercent } from '@/lib/format'
import { useRun } from '@/state/useRun'

/**
 * La descarga en curso, si la hay.
 *
 * Se lee del estado de descargas y no de un aviso del proceso principal porque lo
 * que se quiere mostrar es exactamente lo que ya sabe la interfaz: hay un modelo
 * bajandose y por cuanto va.
 */
function inFlightDownload(downloads: Record<string, ModelDownloadEvent>): ModelDownloadEvent | null {
  for (const event of Object.values(downloads)) {
    if (event.status === 'started' || event.status === 'progress') return event
  }
  return null
}

/** Tiempo transcurrido desde que arranco el lote, actualizado una vez por segundo. */
function useElapsedMs(active: boolean): number {
  const [elapsed, setElapsed] = useState(0)
  const startedAt = useRef<number | null>(null)

  useEffect(() => {
    if (!active) {
      startedAt.current = null
      setElapsed(0)
      return
    }

    startedAt.current = Date.now()
    const timer = setInterval(() => {
      if (startedAt.current !== null) setElapsed(Date.now() - startedAt.current)
    }, 1000)

    return () => clearInterval(timer)
  }, [active])

  return elapsed
}

export function ActionBar() {
  const t = useAtomValue(translatorAtom)
  const hasItems = useAtomValue(hasQueueItemsAtom)
  const stats = useAtomValue(queueStatsAtom)
  const isBusy = useAtomValue(isBusyAtom)
  const phase = useAtomValue(runPhaseAtom)
  const activeJob = useAtomValue(activeJobAtom)
  const downloads = useAtomValue(downloadsAtom)

  const elapsedMs = useElapsedMs(isBusy)
  const { start, cancel, togglePause } = useRun()

  // Entre pulsar «Upscaly» y tener trabajo hay un paso que puede tardar: si el
  // modelo del modo elegido no esta descargado, se descarga antes de aceptar el
  // lote (18 MB el de anime). Sin esta linea, esos segundos son una barra a cero
  // sin explicacion, y parece que la aplicacion se ha colgado.
  const preparing = isBusy && activeJob === null
  const download = preparing ? inFlightDownload(downloads) : null

  const processed = stats.done + stats.degraded + stats.failed
  const remaining = Math.max(0, stats.total - processed)
  const averageMs = processed > 0 ? elapsedMs / processed : 0
  const etaMs = averageMs > 0 ? averageMs * remaining : 0

  const overall = download?.ratio ?? stats.overallProgress

  if (isBusy) {
    return (
      <div className="flex shrink-0 flex-col gap-2 rounded-xl border border-su-border bg-su-surface px-3 py-2.5">
        <div className="flex items-center gap-3">
          <span className="text-[12px] text-su-text">
            {download
              ? t('progress.preparingModel')
              : phase === 'paused'
                ? t('status.pending')
                : t('progress.title')}
          </span>
          <span className="text-[11px] text-su-text-muted">
            {download ? download.modelId : `${processed}/${stats.total}`}
          </span>
          <span className="ml-auto text-[12px] tabular-nums text-su-text">
            {formatPercent(overall)}
          </span>
        </div>

        <Progress value={overall} height={6} ariaLabel={t('progress.global')} />

        <div className="flex items-center gap-2">
          <span className="text-[11px] text-su-text-muted">
            {t('progress.eta')}: {etaMs > 0 ? `~${formatDuration(etaMs)}` : '—'}
          </span>
          <div className="ml-auto flex gap-1.5">
            <Button variant="ghost" size="sm" onClick={togglePause}>
              {phase === 'paused' ? t('action.resume') : t('action.pause')}
            </Button>
            <Button variant="danger" size="sm" onClick={cancel}>
              {t('action.cancel')}
            </Button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <Button variant="primary" size="lg" block disabled={!hasItems} onClick={() => void start()}>
      {t('action.upscaly')}
    </Button>
  )
}
