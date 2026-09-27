import type { ButtonHTMLAttributes } from 'react'
import { cx } from './cx'

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger'
export type ButtonSize = 'sm' | 'md' | 'lg'

const BASE =
  'inline-flex items-center justify-center gap-2 rounded-lg font-medium whitespace-nowrap ' +
  'transition-colors duration-150 select-none ' +
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover ' +
  'disabled:opacity-40 disabled:cursor-not-allowed'

const VARIANT: Record<ButtonVariant, string> = {
  primary: 'bg-su-accent text-white hover:bg-su-accent-hover active:bg-su-accent-muted',
  secondary:
    'bg-su-surface-2 text-su-text border border-su-border hover:border-su-accent-hover hover:text-white',
  ghost: 'bg-transparent text-su-text-muted hover:bg-su-surface-2 hover:text-su-text',
  danger: 'bg-su-error/15 text-su-error border border-su-error/40 hover:bg-su-error/25',
}

const SIZE: Record<ButtonSize, string> = {
  sm: 'h-8 px-3 text-[12px]',
  md: 'h-9 px-4 text-[13px]',
  lg: 'h-12 px-5 text-[15px]',
}

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
  /** Ocupa todo el ancho disponible. */
  block?: boolean
}

export function Button({
  variant = 'secondary',
  size = 'md',
  block = false,
  className,
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cx(BASE, VARIANT[variant], SIZE[size], block && 'w-full', className)}
      {...rest}
    />
  )
}
