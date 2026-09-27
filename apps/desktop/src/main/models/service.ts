/**
 * Servicio de descarga de modelos.
 *
 * Une tres piezas que no se conocen entre sí: el catálogo del sidecar (que dice
 * dónde va cada archivo y con qué hash comprobarlo), el descargador (que es puro
 * y no sabe nada de Electron) y el IPC (que lleva el progreso a la interfaz).
 *
 * Aquí vive lo que sí es responsabilidad del proceso principal:
 *
 *  - **Una descarga por modelo.** Dos clics seguidos escribirían el mismo
 *    `.part` a la vez y el resultado sería basura, así que la segunda petición
 *    se engancha a la primera en lugar de empezar otra.
 *  - **El progreso se emite a todas las ventanas**, no solo a la que lo pidió:
 *    el estado de los modelos es global, y dos ventanas abiertas no pueden
 *    discrepar sobre si un modelo está descargado.
 */

import { join } from 'node:path'
import type { ErrorCode, ModelDownloadEvent } from '@superupscaly/shared'
import { IPC } from '@superupscaly/shared'
import { broadcast } from '../ipc/handle'
import { logger } from '../logging/logger'
import type { SidecarClient } from '../sidecar/client'
import { downloadModel } from './downloader'

interface InFlight {
  controller: AbortController
  done: Promise<ModelDownloadEvent>
}

export class ModelDownloadService {
  private readonly inFlight = new Map<string, InFlight>()

  constructor(private readonly client: SidecarClient) {}

  /**
   * Descarga un modelo. Si ya se está descargando, devuelve la misma promesa.
   *
   * Nunca rechaza: un fallo de red es un resultado, no una excepción, y la
   * interfaz necesita el código de error para poder explicarlo.
   */
  async download(modelId: string): Promise<ModelDownloadEvent> {
    const existing = this.inFlight.get(modelId)
    if (existing) return existing.done

    const controller = new AbortController()
    const done = this.run(modelId, controller).finally(() => {
      this.inFlight.delete(modelId)
    })

    this.inFlight.set(modelId, { controller, done })
    return done
  }

  /** Cancela la descarga en curso de un modelo. `false` si no había ninguna. */
  cancel(modelId: string): boolean {
    const running = this.inFlight.get(modelId)
    if (!running) return false
    running.controller.abort()
    return true
  }

  /** Cancela todo. Se llama al cerrar la aplicación. */
  cancelAll(): void {
    for (const running of this.inFlight.values()) running.controller.abort()
  }

  private async run(
    modelId: string,
    controller: AbortController,
  ): Promise<ModelDownloadEvent> {
    // El destino lo decide el sidecar, que es quien sabe dónde busca los
    // modelos. Calcularlo aquí otra vez sería la forma segura de que las dos
    // rutas se separaran con el tiempo.
    const catalog = await this.client.models().catch((error: unknown) => {
      logger.warn('models.catalog-failed', { message: describe(error) })
      return null
    })

    if (catalog === null) {
      return this.emitFailed(modelId, 'SU-E112', 'no se pudo consultar el catálogo de modelos')
    }

    const model = catalog.models.find((entry) => entry.id === modelId)
    if (!model) {
      return this.emitFailed(modelId, 'SU-E110', `el catálogo no conoce el modelo '${modelId}'`)
    }

    const download = model.download
    if (!download || download.urls.length === 0) {
      return this.emitFailed(
        modelId,
        'SU-E112',
        'este modelo no declara de dónde descargarse',
      )
    }

    // El nombre lo genera el sidecar, pero cruza una frontera: si trajera un
    // separador de ruta, la descarga escribiría fuera del directorio de modelos.
    if (!isPlainFileName(download.fileName)) {
      return this.emitFailed(modelId, 'SU-E161', `nombre de archivo no válido: ${download.fileName}`)
    }

    const destPath = join(catalog.modelsDir, download.fileName)

    this.emit({
      modelId,
      status: 'started',
      receivedBytes: 0,
      totalBytes: download.sizeBytes,
      ratio: null,
      bytesPerSecond: null,
      url: download.urls[0] ?? '',
      errorCode: null,
    })

    const result = await downloadModel({
      modelId,
      urls: download.urls,
      sha256: download.sha256,
      sizeBytes: download.sizeBytes,
      destPath,
      signal: controller.signal,
      onProgress: (progress) => {
        this.emit({
          modelId,
          status: 'progress',
          receivedBytes: progress.receivedBytes,
          totalBytes: progress.totalBytes,
          ratio: progress.ratio,
          bytesPerSecond: progress.bytesPerSecond,
          url: progress.url,
          errorCode: null,
        })
      },
    })

    if (result.status === 'completed') {
      logger.info('models.downloaded', { modelId, bytes: result.bytes })
      return this.emit({
        modelId,
        status: 'completed',
        receivedBytes: result.bytes,
        totalBytes: download.sizeBytes,
        ratio: 1,
        bytesPerSecond: null,
        url: download.urls[0] ?? '',
        errorCode: null,
      })
    }

    if (result.status === 'cancelled') {
      return this.emit({
        modelId,
        status: 'cancelled',
        receivedBytes: 0,
        totalBytes: download.sizeBytes,
        ratio: null,
        bytesPerSecond: null,
        url: download.urls[0] ?? '',
        errorCode: null,
      })
    }

    logger.warn('models.download-failed', { modelId, code: result.errorCode })
    return this.emit({
      modelId,
      status: 'failed',
      receivedBytes: 0,
      totalBytes: download.sizeBytes,
      ratio: null,
      bytesPerSecond: null,
      url: download.urls[0] ?? '',
      errorCode: result.errorCode,
    })
  }

  private emitFailed(modelId: string, errorCode: ErrorCode, message: string): ModelDownloadEvent {
    logger.warn('models.download-rejected', { modelId, code: errorCode, message })
    return this.emit({
      modelId,
      status: 'failed',
      receivedBytes: 0,
      totalBytes: 0,
      ratio: null,
      bytesPerSecond: null,
      url: '',
      errorCode,
    })
  }

  private emit(event: ModelDownloadEvent): ModelDownloadEvent {
    broadcast(IPC.modelDownloadEvent, event)
    return event
  }
}

/**
 * Un nombre de archivo suelto, sin ruta.
 *
 * Se rechaza cualquier separador y `..` para que un catálogo manipulado no pueda
 * escribir fuera del directorio de modelos.
 */
function isPlainFileName(name: string): boolean {
  return (
    name.length > 0 &&
    name.length <= 200 &&
    !name.includes('/') &&
    !name.includes('\\') &&
    !name.includes('..')
  )
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
