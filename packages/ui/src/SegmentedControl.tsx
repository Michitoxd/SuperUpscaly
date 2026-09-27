import type { ReactNode } from 'react'
import { cx } from './cx'

export interface SegmentedOption<T extends string | number> {
  value: T
  label: ReactNode
  /** Texto para lectores de pantalla si `label` es un simbolo. */
  ariaLabel?: string
  disabled?: boolean
}

export interface SegmentedControlProps<T extends string | number> {
  options: ReadonlyArray<SegmentedOption<T>>
  value: T
  onChange: (value: T) => void
  /** Identificador del grupo para `aria-labelledby`. */
  labelId?: string
  className?: string
  size?: 'sm' | 'md'
}

/**
 * Grupo de opciones mutuamente excluyentes. Se implementa con `radiogroup`
 * (no con botones) para que el teclado y los lectores de pantalla se comporten
 * como el usuario espera en un selector de modo o de escala.
 */
export function SegmentedControl<T extends string | number>({
  options,
  value,
  onChange,
  labelId,
  className,
  size = 'md',
}: SegmentedControlProps<T>) {
  const pad = size === 'sm' ? 'h-7 text-[12px]' : 'h-9 text-[13px]'
  return (
    <div role="radiogroup" aria-labelledby={labelId} className={cx('flex gap-1.5', className)}>
      {options.map((option) => {
        const selected = option.value === value
        return (
          <button
            key={String(option.value)}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.ariaLabel}
            disabled={option.disabled}
            onClick={() => onChange(option.value)}
            className={cx(
              'flex-1 rounded-lg border px-3 font-medium transition-colors duration-150',
              pad,
              'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover',
              selected
                ? 'border-su-accent bg-su-accent-muted text-su-text'
                : 'border-su-border bg-su-surface-2 text-su-text-muted hover:text-su-text',
              option.disabled && 'cursor-not-allowed opacity-40 hover:text-su-text-muted',
            )}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
