import type { SuApi } from '@superupscaly/shared'

/**
 * Acceso al puente del preload.
 *
 * Todo el codigo de UI pasa por aqui en lugar de tocar `window.su` directamente.
 * Motivo: la pagina se prerenderiza en el build y puede abrirse fuera de
 * Electron, y en esos casos `window.su` no existe. Un unico punto de guarda
 * evita tener `if (window.su)` repartido por veinte componentes.
 */
export function getBridge(): SuApi | null {
  if (typeof window === 'undefined') return null
  return window.su ?? null
}

export function hasBridge(): boolean {
  return getBridge() !== null
}
