'use client'

import { useCallback } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import {
  isErrorCode,
  type ErrorCode,
  type SidecarJobRequest,
  type TranslationKey,
} from '@superupscaly/shared'
import { normalizeForRun, queueAtom, resetQueueAtom } from '@/state/atoms/queue'
import { settingsAtom, translatorAtom } from '@/state/atoms/settings'
import { outputDirAtom, outputDirStatusAtom } from '@/state/atoms/app'
import { activeJobAtom, sidecarStatusAtom } from '@/state/atoms/sidecar'
import { runPhaseAtom, runSummaryAtom, summaryOpenAtom } from '@/state/atoms/run'
import { pushToastAtom } from '@/state/atoms/ui'
import { getBridge } from '@/lib/bridge'

export interface RunControls {
  start: () => Promise<void>
  cancel: () => void
  togglePause: () => void
}

/**
 * Orquesta un lote contra el sidecar.
 *
 * ## Nada de simulaciones silenciosas
 *
 * Si el motor no esta disponible, se dice y no se hace nada. Hubo una version que
 * simulaba el progreso cuando faltaba el sidecar, y era peor que no hacer nada:
 * el usuario veia una barra avanzar y una carpeta de salida vacia, sin ninguna
 * pista de que el trabajo no se estaba haciendo.
 *
 * ## Los estados no se inventan
 *
 * La interfaz no marca un item como terminado: lo marca el evento que llega del
 * sidecar. Aqui solo se envia la orden y se traduce el resultado.
 */
export function useRun(): RunControls {
  const items = useAtomValue(queueAtom)
  const settings = useAtomValue(settingsAtom)
  const outputDir = useAtomValue(outputDirAtom)
  const outputDirStatus = useAtomValue(outputDirStatusAtom)
  const sidecar = useAtomValue(sidecarStatusAtom)
  const activeJob = useAtomValue(activeJobAtom)
  const phase = useAtomValue(runPhaseAtom)
  const t = useAtomValue(translatorAtom)

  const resetQueue = useSetAtom(resetQueueAtom)
  const setPhase = useSetAtom(runPhaseAtom)
  const setSummary = useSetAtom(runSummaryAtom)
  const setSummaryOpen = useSetAtom(summaryOpenAtom)
  const setActiveJob = useSetAtom(activeJobAtom)
  const pushToast = useSetAtom(pushToastAtom)

  const start = useCallback(async (): Promise<void> => {
    if (activeJob) return

    if (items.length === 0) {
      pushToast({
        tone: 'warning',
        message: t('errors.SU-E001.message'),
        code: 'SU-E001',
        action: t('errors.SU-E001.action'),
      })
      return
    }

    const bridge = getBridge()
    if (!bridge) {
      pushToast({ tone: 'error', message: t('noBridge.body') })
      return
    }

    if (sidecar.state !== 'ready') {
      // El motivo concreto va en el detalle: "no disponible" a secas no le dice
      // al usuario que tiene que compilar el sidecar.
      pushToast({
        tone: 'warning',
        message: t('sidecar.notReady'),
        detail: sidecar.missingBinary ? t('sidecar.missingBinary') : (sidecar.detail ?? undefined),
      })
      return
    }

    if (outputDirStatus === 'invalid') {
      pushToast({
        tone: 'error',
        message: t('errors.SU-E150.message'),
        code: 'SU-E150',
        action: t('errors.SU-E150.action'),
      })
      return
    }

    // La cola vuelve a estado inicial y se envia. Los items se normalizan antes
    // para que un lote repetido no arrastre los tiempos ni los errores del
    // anterior.
    const runnable = items.map(normalizeForRun)
    resetQueue()
    setSummary(null)
    setPhase('running')

    const request: SidecarJobRequest = {
      mode: settings.mode,
      scale: settings.scale,
      items: runnable.map((item) => item.path),
      output: {
        dir: outputDir,
        format: settings.outputFormat,
        quality: settings.outputQuality,
        suffix: settings.suffix,
        preserveMetadata: settings.preserveMetadata,
        zipOutput: false,
      },
      options: {
        tileSize: settings.tileSize,
        device: settings.device,
        concurrency: settings.concurrency,
        unloadBetweenImages: settings.unloadBetweenImages,
        modelChainMode: settings.modelChainMode,
        upscaleModel: settings.upscaleModel.length > 0 ? settings.upscaleModel : null,
        faceRestore: settings.faceRestore,
        denoise: settings.denoise,
        sharpen: settings.sharpen,
      },
      priority: 0,
    }

    try {
      const job = await bridge.createJob(request)
      setActiveJob(job)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)

      // El proceso principal propaga el codigo del sidecar dentro del mensaje
      // (`SU-E110: ...`), asi que se puede distinguir un modelo ausente de un
      // fallo de comunicacion sin inventar una taxonomia nueva.
      const code = extractErrorCode(message)

      setPhase('finished')
      setSummaryOpen(false)

      pushToast({
        tone: 'error',
        message: code ? t(`errors.${code}.message` as TranslationKey) : t('sidecar.jobRejected'),
        ...(code ? { code } : {}),
        detail: message,
      })
    }
  }, [
    activeJob,
    items,
    settings,
    outputDir,
    outputDirStatus,
    sidecar,
    resetQueue,
    setPhase,
    setSummary,
    setSummaryOpen,
    setActiveJob,
    pushToast,
    t,
  ])

  const cancel = useCallback((): void => {
    const bridge = getBridge()
    if (!bridge || !activeJob) return
    // No se cambia el estado local: el sidecar confirma con `jobFinished`, y
    // adelantarse mostraria "cancelado" mientras aun se esta escribiendo un
    // archivo.
    void bridge.cancelJob(activeJob.id).catch(() => undefined)
  }, [activeJob])

  const togglePause = useCallback((): void => {
    const bridge = getBridge()
    if (!bridge || !activeJob) return

    // Se decide por la fase local, no por el estado del trabajo: el objeto que
    // devolvio `createJob` no se refresca con los eventos, asi que su `status`
    // se queda en `queued` para siempre.
    const call = phase === 'paused' ? bridge.resumeJob : bridge.pauseJob
    void call(activeJob.id).catch(() => undefined)
  }, [activeJob, phase])

  return { start, cancel, togglePause }
}

/** Extrae un codigo `SU-Exxx` del mensaje de error, si lo lleva. */
function extractErrorCode(message: string): ErrorCode | null {
  const match = /SU-E\d{3}/.exec(message)
  return match && isErrorCode(match[0]) ? match[0] : null
}
