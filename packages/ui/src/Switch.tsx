import { cx } from './cx'

export interface SwitchProps {
  checked: boolean
  onChange: (checked: boolean) => void
  id?: string
  disabled?: boolean
  label?: string
}

export function Switch({ checked, onChange, id, disabled = false, label }: SwitchProps) {
  return (
    <button
      type="button"
      id={id}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        'relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors duration-150',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover',
        checked ? 'border-su-accent bg-su-accent-muted' : 'border-su-border bg-su-surface-2',
        disabled && 'cursor-not-allowed opacity-40',
      )}
    >
      <span
        className={cx(
          'absolute h-3.5 w-3.5 rounded-full bg-su-text transition-transform duration-150',
          checked ? 'translate-x-[19px]' : 'translate-x-[3px]',
        )}
      />
    </button>
  )
}
