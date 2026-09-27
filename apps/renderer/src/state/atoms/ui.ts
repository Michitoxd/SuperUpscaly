import { atom } from 'jotai'
import type { ErrorCode } from '@superupscaly/shared'

export type ToastTone = 'info' | 'success' | 'warning' | 'error'

export interface Toast {
  id: string
  tone: ToastTone
  /** Texto ya traducido. */
  message: string
  /** Codigo de error, si el aviso corresponde a un fallo del catalogo. */
  code?: ErrorCode
  /** Detalle tecnico expandible (mensaje del sistema, ruta, etc.). */
  detail?: string
  /** Accion sugerida ya traducida. */
  action?: string
}

let toastCounter = 0

export const toastsAtom = atom<Toast[]>([])

export const pushToastAtom = atom(null, (get, set, toast: Omit<Toast, 'id'>): void => {
  toastCounter += 1
  const entry: Toast = { ...toast, id: `toast-${toastCounter.toString(36)}` }
  // Se conservan como maximo cuatro avisos: mas que eso tapa la interfaz.
  set(toastsAtom, [...get(toastsAtom), entry].slice(-4))
})

export const dismissToastAtom = atom(null, (get, set, id: string): void => {
  set(
    toastsAtom,
    get(toastsAtom).filter((toast) => toast.id !== id),
  )
})

/** true si la ventana se abre fuera de Electron; se avisa una sola vez. */
export const standaloneModeAtom = atom(false)
