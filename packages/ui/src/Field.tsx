import type { ReactNode } from 'react'
import { cx } from './cx'
import { Tooltip } from './Tooltip'

export interface FieldProps {
  label: ReactNode
  children: ReactNode
  /** Texto de ayuda; se muestra como icono con tooltip para no ensuciar la UI. */
  hint?: string
  /** Se muestra siempre, no solo en hover. Para avisos que el usuario debe leer. */
  note?: ReactNode
  htmlFor?: string
  className?: string
  /** Distribuye etiqueta y control en la misma linea (para interruptores). */
  inline?: boolean
}

export function Field({ label, children, hint, note, htmlFor, className, inline = false }: FieldProps) {
  return (
    <div className={cx('flex flex-col gap-1.5', className)}>
      <div className={cx('flex items-center gap-1.5', inline && 'justify-between')}>
        <label
          htmlFor={htmlFor}
          className="text-[12px] font-medium text-su-text-muted"
        >
          {label}
        </label>
        {hint ? (
          <Tooltip label={hint}>
            <span
              aria-hidden="true"
              className="flex h-3.5 w-3.5 items-center justify-center rounded-full border border-su-border text-[9px] leading-none text-su-text-muted"
            >
              i
            </span>
          </Tooltip>
        ) : null}
        {inline ? <span className="ml-auto">{children}</span> : null}
      </div>
      {inline ? null : children}
      {note ? <div className="text-[11px] leading-snug text-su-text-muted/80">{note}</div> : null}
    </div>
  )
}
