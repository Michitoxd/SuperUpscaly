'use client'

import { useEffect, useRef } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import type { SidecarEvent, StageId } from '@superupscaly/shared'
import { activeJobAtom, sidecarStatusAtom } from '@/state/atoms/sidecar'
import { patchQueueItemAtom, queueAtom } from '@/state/atoms/queue'
import { runPhaseAtom, runSummaryAtom, summaryOpenAtom } from '@/state/atoms/run'
import { pushToastAtom } from '@/state/atoms/ui'
import { applyDownloadEventAtom, refreshModelsAtom } from '@/state/atoms/models'
import { getBridge } from '@/lib/bridge'

/**
 * Traduce el flujo de eventos del sidecar en estado de interfaz.
 *
 * ## Por que es un hook aparte de `useRun`
 *
 * `useRun` actua (crea, pausa, cancela). Este hook solo escucha. Separarlos evita
 * que la suscripcion se rehaga cada vez que cambia un ajuste, que es lo que
 * pasaria si viviera dentro del hook de ejecucion.
 *
 * ## Como se emparejan los items
 *
 * Los eventos llegan con el identificador que el sidecar asigna a cada imagen. La
 * interfaz tiene los suyos. Se emparejan **por ruta de origen**, que es el unico
 * dato que ambos conocen y que no depende de que los dos lados numeren igual.
 * Emparejar por indice funcionaria hasta que una imagen se saltara por estar ya
 * procesada en una reanudacion, y entonces todo se desplazaria.
 */
export function useSidecarEvents(): void {
  const setStatus = useSetAtom(sidecarStatusAtom)
  const setPhase = useSetAtom(runPhaseAtom)
  const setSummary = useSetAtom(runSummaryAtom)
  const setSummaryOpen = useSetAtom(summaryOpenAtom)
  const setActiveJob = useSetAtom(activeJobAtom)
  const patchItem = useSetAtom(patchQueueItemAtom)
  const pushToast = useSetAtom(pushToastAtom)
  const refreshModels = useSetAtom(refreshModelsAtom)
  const applyDownloadEvent = useSetAtom(applyDownloadEventAtom)

  const queue = useAtomValue(queueAtom)
  const job = useAtomValue(activeJobAtom)

  // Los manejadores necesitan el valor actual sin volver a suscribirse: si
  // dependieran de `queue`, cada cambio de la cola reharia la suscripcion y se
  // perderian eventos en el hueco.
  const queueRef = useRef(queue)
  const jobRef = useRef(job)
  const startedAtRef = useRef<number | null>(null)

  queueRef.current = queue
  jobRef.current = job

  useEffect(() => {
    const bridge = getBridge()
    if (!bridge) return

    // Estado inicial: la ventana puede abrirse con el sidecar ya arrancado.
    void bridge
      .sidecarStatus()
      .then((status) => {
        setStatus(status)

        // Y si ya estaba listo, hay que preguntar el catalogo **aqui**: el aviso de
        // cambio de estado no llegara, porque el motor arranco antes de que esta
        // ventana se suscribiera. Sin esto, la lista de modelos y la cadena de
        // etapas se quedaban vacias hasta que el usuario abria el gestor de
        // modelos, que era la unica otra cosa que las pedia.
        if (status.state === 'ready') void refreshModels()
      })
      .catch(() => {
        // Sin estado se mantiene el valor por defecto, que es "detenido".
      })

    const offStatus = bridge.onSidecarStatus((status) => {
      setStatus(status)
      // El catalogo de modelos solo tiene sentido cuando el motor esta en pie, y
      // puede haber cambiado desde la ultima vez (modelos borrados a mano, otros
      // instalados). Se pregunta al llegar a "listo", no en cada arranque.
      if (status.state === 'ready') void refreshModels()
    })

    // El progreso de las descargas viene por su propio canal: no es un evento
    // del sidecar, sino del proceso principal, que es quien descarga.
    const offDownloads = bridge.onModelDownload((event) => {
      applyDownloadEvent(event)

      // Al terminar una descarga se vuelve a preguntar el catalogo. Importa desde
      // que la aplicacion descarga sola el modelo que falta: sin esto, la barra
      // lateral seguia diciendo «6 sin descargar» despues de haber bajado uno, y
      // el usuario no tenia forma de saber que ya estaba.
      if (event.status === 'completed') void refreshModels()
    })

    const offEvents = bridge.onSidecarEvent((event) => {
      handleEvent(event, {
        findLocalId: (srcPath) =>
          queueRef.current.find((entry) => entry.path === srcPath)?.id ?? null,
        resolvePath: (itemId) =>
          jobRef.current?.items.find((entry) => entry.id === itemId)?.srcPath ?? null,
        patchItem,
        setPhase,
        setSummary,
        setSummaryOpen,
        setActiveJob,
        pushToast,
        startedAtRef,
      })
    })

    return () => {
      offStatus()
      offEvents()
      offDownloads()
    }
  }, [
    setStatus,
    setPhase,
    setSummary,
    setSummaryOpen,
    setActiveJob,
    patchItem,
    pushToast,
    refreshModels,
    applyDownloadEvent,
  ])
}

interface EventContext {
  findLocalId: (srcPath: string) => string | null
  resolvePath: (itemId: string) => string | null
  patchItem: (id: string, patch: Record<string, unknown>) => void
  setPhase: (phase: 'idle' | 'running' | 'paused' | 'finished') => void
  setSummary: (summary: {
    total: number
    done: number
    degraded: number
    failed: number
    totalMs: number
    avgMs: number
    cancelled: boolean
  }) => void
  setSummaryOpen: (open: boolean) => void
  setActiveJob: (job: null) => void
  pushToast: (toast: {
    tone: 'info' | 'success' | 'warning' | 'error'
    message: string
    detail?: string
  }) => void
  startedAtRef: { current: number | null }
}

function handleEvent(event: SidecarEvent, context: EventContext): void {
  switch (event.type) {
    case 'jobStarted': {
      context.startedAtRef.current = Date.now()
      context.setPhase('running')
      return
    }

    case 'itemStarted': {
      const localId = context.findLocalId(context.resolvePath(event.itemId) ?? '')
      if (localId) {
        context.patchItem(localId, { status: 'running', progress: 0, stage: 'decode' })
      }
      return
    }

    case 'itemProgress': {
      const localId = context.findLocalId(context.resolvePath(event.itemId) ?? '')
      if (localId) {
        context.patchItem(localId, {
          progress: event.percent,
          stage: event.stage as StageId,
        })
      }
      return
    }

    case 'itemCompleted': {
      const localId = context.findLocalId(context.resolvePath(event.itemId) ?? '')
      if (localId) {
        context.patchItem(localId, {
          status: event.degraded ? 'degraded' : 'done',
          progress: 1,
          stage: undefined,
          durationMs: event.durationMs,
          degraded: event.degraded,
          // La ruta del resultado, que es lo que permite comparar la imagen
          // original con la escalada sin salir de la aplicacion.
          outPath: event.outPath,
          // Los motivos viajan con el evento desde el principio y hasta ahora se
          // descartaban: la imagen podia salir con otra cadena de etapas y el
          // resumen no decia nada.
          notes: event.skipped.length > 0 ? event.skipped : undefined,
        })
      }
      return
    }

    case 'itemFailed': {
      const localId = context.findLocalId(context.resolvePath(event.itemId) ?? '')
      if (localId) {
        context.patchItem(localId, {
          status: 'failed',
          progress: 0,
          stage: undefined,
          errorCode: event.code,
        })
      }
      return
    }

    case 'jobPaused':
      context.setPhase('paused')
      return

    case 'jobResumed':
      context.setPhase('running')
      return

    case 'jobFinished': {
      const startedAt = context.startedAtRef.current
      const totalMs = startedAt === null ? 0 : Date.now() - startedAt
      const processed = event.done + event.failed

      context.setSummary({
        total: event.done + event.failed + event.degraded,
        done: event.done,
        degraded: event.degraded,
        failed: event.failed,
        totalMs,
        avgMs: processed > 0 ? Math.round(totalMs / processed) : 0,
        cancelled: event.status === 'cancelled',
      })
      context.setPhase('finished')
      context.setSummaryOpen(true)
      context.setActiveJob(null)
      return
    }

    case 'warning': {
      context.pushToast({ tone: 'warning', message: event.message })
      return
    }

    default:
      // Los sucesos informativos (jobCreated, jobProgress) no cambian el estado
      // de los items: el progreso por imagen ya llega en itemProgress.
      return
  }
}
