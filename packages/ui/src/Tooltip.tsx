import type { ReactNode } from 'react'
import { cx } from './cx'

export interface TooltipProps {
  label: ReactNode
  children: ReactNode
  side?: 'top' | 'bottom'
  className?: string
}

/**
 * Ayuda contextual. Se muestra al pasar el raton y al recibir foco por teclado.
 * El texto tambien se expone con `title` para no depender solo del hover.
 */
export function Tooltip({ label, children, side = 'top', className }: TooltipProps) {
  const text = typeof label === 'string' ? label : undefined
  return (
    <span className={cx('group relative inline-flex', className)} title={text}>
      {children}
      <span
        role="tooltip"
        className={cx(
          'pointer-events-none absolute left-0 z-20 w-max max-w-[260px] rounded-lg border border-su-border',
          'bg-su-surface-2 px-2.5 py-1.5 text-[11px] leading-snug text-su-text-muted',
          'opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100',
          side === 'top' ? 'bottom-full mb-2' : 'top-full mt-2',
        )}
      >
        {label}
      </span>
    </span>
  )
}
