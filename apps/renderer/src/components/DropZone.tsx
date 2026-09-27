'use client'

import { useCallback, useRef, useState } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import { Button, cx } from '@superupscaly/ui'
import { queueAtom, hasQueueItemsAtom } from '@/state/atoms/queue'
import { isBusyAtom } from '@/state/atoms/run'
import { translatorAtom } from '@/state/atoms/settings'
import { pushToastAtom } from '@/state/atoms/ui'
import { ingestPaths, mergeIntoQueue, resolveDroppedPaths } from '@/lib/ingest'
import { basename } from '@/lib/format'
import { getBridge } from '@/lib/bridge'
import { QueueList } from './QueueList'

export function DropZone() {
  const t = useAtomValue(translatorAtom)
  const hasItems = useAtomValue(hasQueueItemsAtom)
  const isBusy = useAtomValue(isBusyAtom)
  const setQueue = useSetAtom(queueAtom)
  const pushToast = useSetAtom(pushToastAtom)

  const [dragging, setDragging] = useState(false)
  // Contador en lugar de booleano: al arrastrar sobre un hijo se dispara
  // `dragleave` en el padre, y un booleano simple haria parpadear el resaltado.
  const dragDepth = useRef(0)

  const addPaths = useCallback(
    async (paths: string[], extra: { unresolved: number; overLimit: number }): Promise<void> => {
      if (extra.overLimit > 0) {
        pushToast({
          tone: 'warning',
          message: t('drop.limitExceeded', { count: extra.overLimit, max: 5000 }),
        })
      }

      if (extra.unresolved > 0) {
        pushToast({
          tone: 'warning',
          message: t('drop.rejected', { count: extra.unresolved }),
          code: 'SU-E161',
          action: t('drop.rejectedAction'),
        })
      }

      if (paths.length === 0) return

      const result = await ingestPaths(paths)

      if (result.noBridge) {
        pushToast({ tone: 'error', message: t('noBridge.body') })
        return
      }

      if (result.rejected.length > 0) {
        const reasons = [...new Set(result.rejected.map((entry) => entry.reason))].join(', ')
        pushToast({
          tone: 'warning',
          message: t('drop.rejected', { count: result.rejected.length }),
          detail: reasons,
          action: t('drop.rejectedAction'),
        })
      }

      // Un comprimido que no se pudo abrir deja su fila en la cola y se cuenta
      // aqui, al encolar: esperar a pulsar Upscaly para decirlo obligaba a
      // arrastrar, ordenar y lanzar un lote que ya se sabia que no iba a salir.
      if (result.archivesFailed.length > 0) {
        pushToast({
          tone: 'warning',
          message: t('drop.archiveFailed', { count: result.archivesFailed.length }),
          code: result.archivesFailed[0]?.errorCode,
          detail: result.archivesFailed
            .map((failure) => `${basename(failure.path)}: ${failure.message}`)
            .join('; '),
          action: t('drop.rejectedAction'),
        })
      }

      if (result.items.length === 0) {
        if (result.rejected.length === 0) {
          pushToast({ tone: 'warning', message: t('errors.SU-E001.message'), code: 'SU-E001' })
        }
        return
      }

      setQueue((previous) => mergeIntoQueue(previous, result.items))
    },
    [pushToast, setQueue, t],
  )

  const handleDrop = useCallback(
    (event: React.DragEvent<HTMLDivElement>): void => {
      event.preventDefault()
      event.stopPropagation()
      dragDepth.current = 0
      setDragging(false)

      const transfer = event.dataTransfer
      if (!transfer || transfer.files.length === 0) {
        // Texto, enlaces o elementos de una pagina: se ignoran sin ruido.
        return
      }

      // Sincrono y sin await: en cuanto este manejador devuelve, el
      // DataTransfer se invalida y las rutas se pierden.
      const resolved = resolveDroppedPaths(Array.from(transfer.files))
      void addPaths(resolved.paths, { unresolved: resolved.unresolved, overLimit: resolved.overLimit })
    },
    [addPaths],
  )

  const openFileDialog = useCallback(async (): Promise<void> => {
    const bridge = getBridge()
    if (!bridge) {
      pushToast({ tone: 'error', message: t('noBridge.body') })
      return
    }
    const picked = await bridge.pickImages()
    if (picked.length > 0) await addPaths(picked, { unresolved: 0, overLimit: 0 })
  }, [addPaths, pushToast, t])

  return (
    <div
      onDragEnter={(event) => {
        event.preventDefault()
        dragDepth.current += 1
        if (!isBusy) setDragging(true)
      }}
      onDragOver={(event) => {
        event.preventDefault()
        event.stopPropagation()
        if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy'
      }}
      onDragLeave={(event) => {
        event.preventDefault()
        dragDepth.current = Math.max(0, dragDepth.current - 1)
        if (dragDepth.current === 0) setDragging(false)
      }}
      onDrop={handleDrop}
      className={cx(
        'flex min-h-0 flex-1 flex-col rounded-xl border border-dashed transition-colors duration-150',
        dragging ? 'border-su-accent-hover bg-su-accent/10' : 'border-su-border bg-su-surface',
      )}
    >
      {hasItems ? (
        <QueueList />
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
          <div
            aria-hidden="true"
            className={cx(
              'flex h-12 w-12 items-center justify-center rounded-xl border text-[18px] transition-colors',
              dragging ? 'border-su-accent-hover text-su-accent-hover' : 'border-su-border text-su-text-muted',
            )}
          >
            ↓
          </div>
          <p className="text-[14px] text-su-text">{dragging ? t('drop.dragging') : t('drop.title')}</p>
          <p className="text-[11px] text-su-text-muted">{t('drop.hint')}</p>
          <Button variant="secondary" onClick={() => void openFileDialog()} className="mt-1">
            {t('drop.button')}
          </Button>
        </div>
      )}
    </div>
  )
}
