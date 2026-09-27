import { BrowserWindow, ipcMain, type IpcMainInvokeEvent } from 'electron'
import { logger } from '../logging/logger'

/**
 * Registro de manejadores IPC con las dos comprobaciones que deben ser
 * inevitables, no opcionales por manejador:
 *
 * 1. **Solo se atiende al webContents de una ventana propia.** Un iframe
 *    incrustado o una ventana secundaria no pueden usar el puente.
 * 2. **Todo error se registra.** Un `throw` dentro de un manejador llega al
 *    renderer como una promesa rechazada sin contexto; sin este registro, el
 *    motivo real se pierde.
 */
export function isTrustedSender(event: IpcMainInvokeEvent): boolean {
  return BrowserWindow.getAllWindows().some(
    (win) => !win.isDestroyed() && win.webContents.id === event.sender.id,
  )
}

export function handle<TArgs extends unknown[], TResult>(
  channel: string,
  handler: (...args: TArgs) => Promise<TResult> | TResult,
): void {
  ipcMain.handle(channel, async (event, ...args) => {
    if (!isTrustedSender(event)) {
      logger.warn('ipc.untrusted-sender', { channel })
      throw new Error('IPC request from an untrusted sender')
    }

    try {
      return await handler(...(args as TArgs))
    } catch (error) {
      logger.warn('ipc.handler-failed', {
        channel,
        message: error instanceof Error ? error.message : String(error),
      })
      // Se propaga el mensaje original: la interfaz lo necesita para mostrar el
      // codigo de error del sidecar en lugar de un texto generico.
      throw error
    }
  })
}

/** Envia un mensaje a todas las ventanas vivas. */
export function broadcast(channel: string, payload: unknown): void {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) {
      win.webContents.send(channel, payload)
    }
  }
}
