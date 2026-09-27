'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { cx } from '@superupscaly/ui'
import { dismissToastAtom, toastsAtom, type ToastTone } from '@/state/atoms/ui'
import { translatorAtom } from '@/state/atoms/settings'
import { getBridge } from '@/lib/bridge'

const TONE_BORDER: Record<ToastTone, string> = {
  info: 'border-su-border',
  success: 'border-su-success/50',
  warning: 'border-su-warning/50',
  error: 'border-su-error/50',
}

const TONE_TEXT: Record<ToastTone, string> = {
  info: 'text-su-text',
  success: 'text-su-success',
  warning: 'text-su-warning',
  error: 'text-su-error',
}

export function Toaster() {
  const t = useAtomValue(translatorAtom)
  const toasts = useAtomValue(toastsAtom)
  const dismiss = useSetAtom(dismissToastAtom)

  if (toasts.length === 0) return null

  return (
    <div className="pointer-events-none absolute right-4 bottom-4 z-40 flex w-[380px] flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role={toast.tone === 'error' ? 'alert' : 'status'}
          className={cx(
            'pointer-events-auto rounded-xl border bg-su-surface px-3 py-2.5',
            TONE_BORDER[toast.tone],
          )}
        >
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <p className={cx('text-[12px] leading-snug', TONE_TEXT[toast.tone])}>{toast.message}</p>

              {toast.action ? (
                <p className="mt-1 text-[11px] leading-snug text-su-text-muted">
                  <span className="text-su-text-muted/70">{t('error.actionLabel')}: </span>
                  {toast.action}
                </p>
              ) : null}

              {toast.detail ? (
                <details className="mt-1">
                  <summary className="cursor-pointer text-[10px] text-su-text-muted/70">
                    {t('error.details')}
                  </summary>
                  <pre className="mt-1 max-h-[120px] overflow-auto rounded-md bg-su-surface-2 p-2 text-[10px] whitespace-pre-wrap text-su-text-muted">
                    {toast.detail}
                  </pre>
                </details>
              ) : null}
            </div>

            <button
              type="button"
              aria-label={t('common.close')}
              onClick={() => dismiss(toast.id)}
              className="shrink-0 rounded-md px-1.5 text-[13px] leading-none text-su-text-muted/60 transition-colors hover:text-su-text"
            >
              ×
            </button>
          </div>

          {toast.tone === 'error' ? (
            <button
              type="button"
              onClick={() => {
                const bridge = getBridge()
                if (bridge) void bridge.openLogsFolder()
              }}
              className="mt-1.5 text-[10px] text-su-text-muted/70 underline underline-offset-2 hover:text-su-text"
            >
              {t('error.openLogs')}
            </button>
          ) : null}
        </div>
      ))}
    </div>
  )
}
