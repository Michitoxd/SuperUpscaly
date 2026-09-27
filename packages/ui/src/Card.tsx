import type { HTMLAttributes, ReactNode } from 'react'
import { cx } from './cx'

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode
  /** Padding reducido para listas densas. */
  compact?: boolean
}

export function Card({ children, compact = false, className, ...rest }: CardProps) {
  return (
    <div
      className={cx(
        'rounded-xl border border-su-border bg-su-surface',
        compact ? 'p-2' : 'p-3',
        className,
      )}
      {...rest}
    >
      {children}
    </div>
  )
}

export interface SectionLabelProps {
  children: ReactNode
  className?: string
  /** Permite que un `radiogroup` se anuncie con esta etiqueta. */
  id?: string
}

/** Etiqueta de seccion en la barra lateral. */
export function SectionLabel({ children, className, id }: SectionLabelProps) {
  return (
    <div
      id={id}
      className={cx('mb-2 text-[11px] font-medium tracking-wide text-su-text-muted', className)}
    >
      {children}
    </div>
  )
}
