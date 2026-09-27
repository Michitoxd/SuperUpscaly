import { cx } from './cx'

export type ProgressTone = 'accent' | 'success' | 'warning' | 'error'

const TONE: Record<ProgressTone, string> = {
  accent: 'bg-su-accent',
  success: 'bg-su-success',
  warning: 'bg-su-warning',
  error: 'bg-su-error',
}

export interface ProgressProps {
  /** 0..1. Valores fuera de rango se recortan. */
  value: number
  tone?: ProgressTone
  /** Altura en px. 6 por defecto. */
  height?: number
  className?: string
  ariaLabel?: string
}

export function Progress({ value, tone = 'accent', height = 6, className, ariaLabel }: ProgressProps) {
  const pct = Math.round(Math.min(1, Math.max(0, value)) * 100)
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      aria-label={ariaLabel}
      className={cx('w-full overflow-hidden rounded-full bg-su-surface-2', className)}
      style={{ height }}
    >
      <div
        className={cx('h-full rounded-full transition-[width] duration-200 ease-out', TONE[tone])}
        style={{ width: `${pct}%` }}
      />
    </div>
  )
}
