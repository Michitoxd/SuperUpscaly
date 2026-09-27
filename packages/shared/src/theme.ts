/**
 * Paleta oficial de SuperUpscaly.
 *
 * Fuente unica de verdad: `globals.css` consume estos mismos valores a traves de
 * las variables CSS `--su-*`, y cualquier componente que necesite el color en
 * TypeScript (graficas, canvas, estilos inline calculados) lo toma de aqui.
 *
 * Regla: ningun componente debe contener un hex literal. Si hace falta un color
 * nuevo, se anade aqui y se refleja en `globals.css`.
 */

export const palette = {
  bgBase: '#1E1B2E',
  bgSurface: '#2D2640',
  bgSurface2: '#382F52',
  accent: '#8B5CF6',
  accentHover: '#A78BFA',
  accentMuted: '#6D28D9',
  textPrimary: '#F3F4F6',
  textSecondary: '#C4B5FD',
  border: '#4C1D95',
  success: '#34D399',
  warning: '#FBBF24',
  error: '#F87171',
} as const

export type PaletteToken = keyof typeof palette

/**
 * Mapa token -> variable CSS generada por Tailwind v4 a partir de `@theme`.
 * El prefijo es `--color-su-*` porque es lo que Tailwind emite en `:root`.
 */
export const cssVar: Record<PaletteToken, string> = {
  bgBase: 'var(--color-su-base)',
  bgSurface: 'var(--color-su-surface)',
  bgSurface2: 'var(--color-su-surface-2)',
  accent: 'var(--color-su-accent)',
  accentHover: 'var(--color-su-accent-hover)',
  accentMuted: 'var(--color-su-accent-muted)',
  textPrimary: 'var(--color-su-text)',
  textSecondary: 'var(--color-su-text-muted)',
  border: 'var(--color-su-border)',
  success: 'var(--color-su-success)',
  warning: 'var(--color-su-warning)',
  error: 'var(--color-su-error)',
}

/** Estado de un item de la cola -> color de la paleta. */
export const statusColor = {
  pending: palette.textSecondary,
  running: palette.accentHover,
  done: palette.success,
  degraded: palette.warning,
  failed: palette.error,
  skipped: palette.textSecondary,
} as const
