'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { Button } from '@superupscaly/ui'
import { runSummaryAtom, summaryOpenAtom } from '@/state/atoms/run'
import { queueAtom } from '@/state/atoms/queue'
import { translatorAtom } from '@/state/atoms/settings'
import { outputDirAtom } from '@/state/atoms/app'
import { getBridge } from '@/lib/bridge'
import { formatDuration } from '@/lib/format'

interface MetricProps {
  label: string
  value: string
  tone?: string
}

function Metric({ label, value, tone }: MetricProps) {
  return (
    <div className="rounded-lg bg-su-surface-2 px-3 py-2">
      <div className="text-[11px] text-su-text-muted">{label}</div>
      <div className="mt-0.5 text-[20px] font-medium tabular-nums" style={tone ? { color: tone } : undefined}>
        {value}
      </div>
    </div>
  )
}

export function SummaryDialog() {
  const t = useAtomValue(translatorAtom)
  const summary = useAtomValue(runSummaryAtom)
  const open = useAtomValue(summaryOpenAtom)
  const outputDir = useAtomValue(outputDirAtom)
  const items = useAtomValue(queueAtom)
  const setOpen = useSetAtom(summaryOpenAtom)

  // Solo las imagenes que salieron distintas de lo configurado. El motor calcula
  // estos motivos desde el primer dia; hasta ahora se quedaban en la respuesta de
  // la API, sin llegar nunca a la pantalla.
  const withNotes = items.filter((item) => item.notes && item.notes.length > 0)

  if (!open || !summary) return null

  const reveal = (): void => {
    const bridge = getBridge()
    if (bridge && outputDir.length > 0) void bridge.revealInFolder(outputDir)
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('summary.title')}
      className="absolute inset-0 z-30 flex items-center justify-center bg-su-base/70 p-6"
      onClick={() => setOpen(false)}
    >
      <div
        className="w-full max-w-[460px] rounded-xl border border-su-border bg-su-surface p-4"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="mb-3 flex items-center gap-2">
          <h2 className="text-[15px] font-medium text-su-text">{t('summary.title')}</h2>
          {summary.cancelled ? (
            <span className="rounded-md border border-su-warning/40 bg-su-warning/10 px-1.5 py-0.5 text-[10px] text-su-warning">
              {t('errors.SU-E160.message')}
            </span>
          ) : null}
        </div>

        <div className="grid grid-cols-2 gap-2">
          <Metric label={t('summary.success')} value={String(summary.done)} tone="var(--color-su-success)" />
          <Metric
            label={t('summary.failed')}
            value={String(summary.failed)}
            tone={summary.failed > 0 ? 'var(--color-su-error)' : undefined}
          />
          <Metric label={t('summary.totalTime')} value={formatDuration(summary.totalMs)} />
          <Metric label={t('summary.avgTime')} value={summary.avgMs > 0 ? formatDuration(summary.avgMs) : '—'} />
        </div>

        {summary.degraded > 0 ? (
          <p className="mt-3 text-[11px] text-su-warning">
            {t('summary.degraded')}: {summary.degraded}
          </p>
        ) : null}

        {withNotes.length > 0 ? (
          <div className="mt-4 max-h-[180px] overflow-y-auto rounded-lg bg-su-surface-2 p-2">
            <div className="mb-1 text-[11px] text-su-text-muted">{t('summary.notes')}</div>
            <p className="mb-2 text-[10px] text-su-text-muted/70">{t('summary.notesHint')}</p>
            <ul className="space-y-1.5">
              {withNotes.map((item) => (
                <li key={item.id} className="text-[11px]">
                  <div className="truncate text-su-text" title={item.path}>
                    {item.name}
                  </div>
                  <ul className="mt-0.5 space-y-0.5 pl-3">
                    {item.notes?.map((note) => (
                      <li key={note} className="text-[10px] text-su-text-muted">
                        · {note}
                      </li>
                    ))}
                  </ul>
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        <div className="mt-4 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={() => setOpen(false)}>
            {t('common.close')}
          </Button>
          <Button variant="secondary" size="sm" onClick={reveal} disabled={outputDir.length === 0}>
            {t('action.openOutput')}
          </Button>
        </div>
      </div>
    </div>
  )
}
