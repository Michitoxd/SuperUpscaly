'use client'

import { useEffect } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import { Button, Progress, cx } from '@superupscaly/ui'
import type {
  ModelDownloadEvent,
  ModelState,
  SidecarModelStatus,
  TranslationKey,
} from '@superupscaly/shared'
import { modelsAtom, modelsDirAtom } from '@/state/atoms/sidecar'
import {
  cancelDownloadAtom,
  downloadsAtom,
  modelsOpenAtom,
  refreshModelsAtom,
  startDownloadAtom,
} from '@/state/atoms/models'
import { translatorAtom } from '@/state/atoms/settings'
import { getBridge } from '@/lib/bridge'
import { formatBytes } from '@/lib/format'

/**
 * Gestor de modelos.
 *
 * El catalogo se muestra **siempre entero**, incluidos los modelos que ya estan
 * instalados. Esconder los que no hacen falta deja al usuario sin forma de saber
 * que existen, y la pregunta "¿por que el modo dibujo usa este modelo y no
 * aquel?" solo se responde viendo la lista completa.
 *
 * La licencia se muestra **antes** de descargar, no despues: dos de los modelos
 * del catalogo no permiten uso comercial, y enterarse al terminar una descarga
 * de 340 MB es tarde.
 */

const STATE_TONE: Record<ModelState, string> = {
  installed: 'border-su-success/40 bg-su-success/10 text-su-success',
  missing: 'border-su-border bg-su-surface-2 text-su-text-muted',
  hashMismatch: 'border-su-error/40 bg-su-error/10 text-su-error',
  unverified: 'border-su-warning/40 bg-su-warning/10 text-su-warning',
}

const STATE_KEY: Record<ModelState, TranslationKey> = {
  installed: 'models.state.installed',
  missing: 'models.state.missing',
  hashMismatch: 'models.state.hashMismatch',
  unverified: 'models.state.unverified',
}

const KIND_KEY: Record<SidecarModelStatus['kind'], TranslationKey> = {
  photo: 'models.kind.photo',
  illustration: 'models.kind.illustration',
  denoise: 'models.kind.denoise',
  face: 'models.kind.face',
  detector: 'models.kind.detector',
  classifier: 'models.kind.classifier',
}

export function ModelsDialog() {
  const t = useAtomValue(translatorAtom)
  const open = useAtomValue(modelsOpenAtom)
  const models = useAtomValue(modelsAtom)
  const modelsDir = useAtomValue(modelsDirAtom)
  const downloads = useAtomValue(downloadsAtom)

  const setOpen = useSetAtom(modelsOpenAtom)
  const refresh = useSetAtom(refreshModelsAtom)
  const startDownload = useSetAtom(startDownloadAtom)
  const cancelDownload = useSetAtom(cancelDownloadAtom)

  // Al abrir se pregunta otra vez: el catalogo pudo cambiar (un modelo
  // descargado a mano, un archivo borrado desde el explorador).
  useEffect(() => {
    if (open) void refresh()
  }, [open, refresh])

  if (!open) return null

  const missing = models.filter((model) => model.state !== 'installed').length

  const revealFolder = (): void => {
    const bridge = getBridge()
    if (bridge && modelsDir.length > 0) void bridge.revealInFolder(modelsDir)
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('models.title')}
      className="absolute inset-0 z-30 flex items-center justify-center bg-su-base/70 p-6"
      onClick={() => setOpen(false)}
    >
      <div
        className="flex max-h-full w-full max-w-[560px] flex-col rounded-xl border border-su-border bg-su-surface p-4"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="mb-1 flex items-center gap-2">
          <h2 className="text-[15px] font-medium text-su-text">{t('models.title')}</h2>
          {missing > 0 ? (
            <span className="rounded-md border border-su-warning/40 bg-su-warning/10 px-1.5 py-0.5 text-[10px] text-su-warning">
              {t('models.missingCount', { count: missing })}
            </span>
          ) : null}
        </div>
        <p className="mb-3 text-[11px] text-su-text-muted">{t('models.subtitle')}</p>

        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto">
          {models.length === 0 ? (
            <p className="py-6 text-center text-[12px] text-su-text-muted">{t('models.empty')}</p>
          ) : (
            models.map((model) => (
              <ModelRow
                key={model.id}
                model={model}
                download={downloads[model.id]}
                onDownload={() => void startDownload(model.id)}
                onCancel={() => void cancelDownload(model.id)}
              />
            ))
          )}
        </div>

        <div className="mt-3 flex items-center justify-between gap-2 border-t border-su-border/60 pt-3">
          <span className="min-w-0 truncate text-[10px] text-su-text-muted/70" title={modelsDir}>
            {modelsDir}
          </span>
          <div className="flex shrink-0 gap-2">
            <Button variant="ghost" size="sm" onClick={revealFolder} disabled={modelsDir.length === 0}>
              {t('models.openFolder')}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setOpen(false)}>
              {t('common.close')}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

interface ModelRowProps {
  model: SidecarModelStatus
  download: ModelDownloadEvent | undefined
  onDownload: () => void
  onCancel: () => void
}

function ModelRow({ model, download, onDownload, onCancel }: ModelRowProps) {
  const t = useAtomValue(translatorAtom)

  // Solo hay descarga en curso mientras el ultimo evento diga eso: al terminar,
  // el evento final se queda guardado y la fila vuelve a su estado normal.
  const running = download?.status === 'started' || download?.status === 'progress'
  const expected = model.download?.sizeBytes ?? 0

  return (
    <div className="rounded-lg border border-su-border bg-su-surface-2 px-3 py-2">
      <div className="flex items-center gap-2">
        <span className="truncate text-[12px] text-su-text">{model.name}</span>
        <span className="shrink-0 text-[10px] text-su-text-muted">
          {t(KIND_KEY[model.kind])} · {model.scale}x
        </span>
        <span
          className={cx(
            'ml-auto shrink-0 rounded-md border px-1.5 py-0.5 text-[10px]',
            STATE_TONE[model.state],
          )}
        >
          {t(STATE_KEY[model.state])}
        </span>
      </div>

      <div className="mt-1 flex items-center gap-2">
        <span className="text-[10px] text-su-text-muted/70">
          {expected > 0 ? formatBytes(expected) : t('models.sizeUnknown')}
        </span>

        {model.licenseWarning ? (
          <span className="truncate text-[10px] text-su-warning" title={model.licenseWarning}>
            {model.licenseWarning}
          </span>
        ) : null}
      </div>

      {running ? (
        <div className="mt-2 flex items-center gap-2">
          <Progress
            value={download.ratio ?? 0}
            tone="accent"
            height={4}
            ariaLabel={t('models.downloading')}
            className="flex-1"
          />
          <span className="shrink-0 text-[10px] tabular-nums text-su-text-muted">
            {download.ratio === null ? t('models.downloading') : `${Math.round(download.ratio * 100)}%`}
          </span>
          <Button variant="ghost" size="sm" onClick={onCancel}>
            {t('common.cancel')}
          </Button>
        </div>
      ) : model.download ? (
        <div className="mt-2">
          <Button variant="secondary" size="sm" onClick={onDownload}>
            {model.state === 'hashMismatch' ? t('models.redownload') : t('models.download')}
          </Button>
        </div>
      ) : model.state !== 'installed' ? (
        // Ni descargable ni instalado: el manifiesto no declara de donde traerlo.
        // Se dice, en lugar de ofrecer un boton que no puede funcionar.
        <p className="mt-2 text-[10px] text-su-text-muted/70">{t('models.noDownload')}</p>
      ) : null}
    </div>
  )
}
