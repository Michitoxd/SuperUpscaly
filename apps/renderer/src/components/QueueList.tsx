'use client'

import { useCallback } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import { Button, Progress, cx } from '@superupscaly/ui'
import { statusColor, type ItemStatus, type TranslationKey } from '@superupscaly/shared'
import { compareItemIdAtom } from '@/state/atoms/compare'
import { clearQueueAtom, queueAtom, queueStatsAtom, removeFromQueueAtom } from '@/state/atoms/queue'
import { isBusyAtom } from '@/state/atoms/run'
import { translatorAtom } from '@/state/atoms/settings'
import { pushToastAtom } from '@/state/atoms/ui'
import { getBridge } from '@/lib/bridge'
import { formatBytes, formatDuration, formatPercent } from '@/lib/format'
import { ingestPaths, mergeIntoQueue } from '@/lib/ingest'

const STATUS_KEY: Record<ItemStatus, TranslationKey> = {
  pending: 'status.pending',
  running: 'status.running',
  done: 'status.done',
  degraded: 'status.degraded',
  failed: 'status.failed',
  skipped: 'status.skipped',
}

export function QueueList() {
  const t = useAtomValue(translatorAtom)
  const items = useAtomValue(queueAtom)
  const stats = useAtomValue(queueStatsAtom)
  const isBusy = useAtomValue(isBusyAtom)
  const setQueue = useSetAtom(queueAtom)
  const clearQueue = useSetAtom(clearQueueAtom)
  const removeItem = useSetAtom(removeFromQueueAtom)
  const setCompareItemId = useSetAtom(compareItemIdAtom)
  const pushToast = useSetAtom(pushToastAtom)

  const addMore = useCallback(async (): Promise<void> => {
    const bridge = getBridge()
    if (!bridge) {
      pushToast({ tone: 'error', message: t('noBridge.body') })
      return
    }
    const picked = await bridge.pickImages()
    if (picked.length === 0) return
    const result = await ingestPaths(picked)
    if (result.items.length > 0) {
      setQueue((previous) => mergeIntoQueue(previous, result.items))
    }
  }, [pushToast, setQueue, t])

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-su-border px-3 py-2">
        <span className="text-[12px] text-su-text-muted">
          {stats.total} · {formatBytes(stats.totalBytes)}
        </span>
        <div className="ml-auto flex gap-1.5">
          <Button variant="ghost" size="sm" disabled={isBusy} onClick={() => void addMore()}>
            {t('drop.button')}
          </Button>
          <Button variant="ghost" size="sm" disabled={isBusy} onClick={() => clearQueue()}>
            {t('queue.clear')}
          </Button>
        </div>
      </div>

      <ul className="min-h-0 flex-1 overflow-y-auto">
        {items.map((item) => {
          const tone = statusColor[item.status]
          return (
            <li
              key={item.id}
              className="group flex items-center gap-3 border-b border-su-border/50 px-3 py-2 last:border-b-0"
            >
              <span
                aria-hidden="true"
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: tone }}
              />

              <div className="min-w-0 flex-1">
                <div className="truncate text-[12px] text-su-text" title={item.path}>
                  {item.name}
                </div>
                <div className="truncate text-[10px] text-su-text-muted/70" title={item.path}>
                  {item.dir}
                </div>
              </div>

              <div className="w-[64px] shrink-0 text-right text-[11px] tabular-nums text-su-text-muted">
                {formatBytes(item.sizeBytes)}
              </div>

              <div className="w-[92px] shrink-0 text-[11px]" style={{ color: tone }}>
                {t(STATUS_KEY[item.status])}
                {item.stage ? (
                  <span className="block text-[10px] text-su-text-muted/70">
                    {t(`progress.stage.${item.stage}` as TranslationKey)}
                  </span>
                ) : null}
              </div>

              {/* Una imagen termino distinta de lo configurado: el motivo esta en el
                  resumen, aqui solo se avisa de que hay algo que leer. */}
              {item.notes && item.notes.length > 0 ? (
                <span
                  aria-label={t('summary.notes')}
                  title={item.notes.join('\n')}
                  className="shrink-0 rounded-md border border-su-warning/40 bg-su-warning/10 px-1 py-0.5 text-[10px] leading-none text-su-warning"
                >
                  {item.notes.length}
                </span>
              ) : null}

              <div className="w-[110px] shrink-0">
                {item.status === 'failed' && item.errorCode ? (
                  <span className="text-[11px] text-su-error" title={t(`errors.${item.errorCode}.message` as TranslationKey)}>
                    {item.errorCode}
                  </span>
                ) : (
                  <div className="flex items-center gap-2">
                    <Progress
                      value={item.progress}
                      tone={item.status === 'done' ? 'success' : item.status === 'degraded' ? 'warning' : 'accent'}
                      height={4}
                      ariaLabel={item.name}
                    />
                    <span className="w-[34px] shrink-0 text-right text-[10px] tabular-nums text-su-text-muted">
                      {formatPercent(item.progress)}
                    </span>
                  </div>
                )}
              </div>

              <div className="w-[62px] shrink-0 text-right text-[11px] tabular-nums text-su-text-muted">
                {item.durationMs !== undefined ? formatDuration(item.durationMs) : '—'}
              </div>

              {/* Comparar solo tiene sentido cuando existe el resultado: el boton
                  aparece con el, en lugar de estar ahi y no hacer nada. */}
              {item.outPath !== undefined ? (
                <button
                  type="button"
                  aria-label={`${t('compare.open')} ${item.name}`}
                  title={t('compare.open')}
                  onClick={() => setCompareItemId(item.id)}
                  className="shrink-0 rounded-md px-1.5 py-0.5 text-[13px] leading-none text-su-text-muted/70 transition-colors hover:bg-su-surface-2 hover:text-su-accent-hover"
                >
                  &#8596;
                </button>
              ) : null}

              <button
                type="button"
                aria-label={`${t('common.remove')} ${item.name}`}
                disabled={isBusy}
                onClick={() => removeItem(item.id)}
                className={cx(
                  'shrink-0 rounded-md px-1.5 py-0.5 text-[13px] leading-none text-su-text-muted/50 transition-colors',
                  'hover:bg-su-surface-2 hover:text-su-error disabled:opacity-0',
                )}
              >
                ×
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
