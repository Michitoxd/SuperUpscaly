import { basename } from 'node:path'

import {
  IPC,
  isSafePathString,
  type ModelDownloadEvent,
  type SidecarCapabilities,
  type SidecarJob,
  type SidecarJobRequest,
  type SidecarModelsResult,
  type SidecarPipelinesResult,
  type SidecarStatus,
} from '@superupscaly/shared'
import type { SidecarClient } from '../sidecar/client'
import type { SidecarSupervisor } from '../sidecar/supervisor'
import { ModelDownloadService } from '../models/service'
import { ensureModelsFor } from '../models/ensure'
import { ArchiveService, type ArchiveFailure } from '../archives/service'
import { logger } from '../logging/logger'
import { registerMediaPath } from '../media/registry'
import { broadcast, handle } from './handle'

/**
 * Puente entre la interfaz y el sidecar.
 *
 * ## Validacion
 *
 * A diferencia del resto de la frontera IPC, aqui **se lanza un error** en lugar
 * de descartar los campos invalidos en silencio. El motivo es concreto: un
 * ajuste de la aplicacion que se descarta no cambia nada visible, pero un campo
 * de un trabajo que se descarta hace que el trabajo haga algo distinto de lo que
 * el usuario pidio. Es mejor que falle ahora y con un mensaje claro.
 *
 * ## Eventos
 *
 * Los eventos del sidecar se reenvian a todas las ventanas. Se hace en un unico
 * sitio, al registrar los manejadores, para que no dependa de que alguien se
 * acuerde de suscribirse.
 */
export function registerSidecarIpc(
  supervisor: SidecarSupervisor,
  client: SidecarClient,
): ModelDownloadService {
  handle<[], SidecarStatus>(IPC.sidecarStatus, () => supervisor.getStatus())

  handle<[], SidecarCapabilities>(IPC.sidecarCapabilities, () => client.capabilities())

  handle<[], SidecarModelsResult>(IPC.sidecarModels, () => client.models())

  handle<[], SidecarPipelinesResult>(IPC.sidecarPipelines, () => client.pipelines())

  const archives = new ArchiveService()

  // Se construye antes del manejador de trabajos porque crear un trabajo implica
  // descargar el modelo que falte (ver `models/ensure.ts`).
  const models = new ModelDownloadService(client)

  // Los ZIP y CBZ se expanden aqui, antes de encolar: el sidecar trabaja con
  // rutas de imagen. Si un archivo comprimido no se puede abrir se lanza, en vez
  // de encolar un trabajo al que le falta un archivo que el usuario si pidio.
  // Es la misma politica que el resto del modulo (ver la cabecera).
  handle<[unknown], SidecarJob>(IPC.sidecarCreateJob, async (raw) => {
    const request = sanitizeJobRequest(raw)
    const expandido = await archives.expandAll(request.items)

    if (expandido.failed.length > 0) {
      throw new Error(describirFallosDeArchivo(expandido.failed))
    }

    // Antes de aceptar el trabajo: si el modelo que la etapa de escalado necesita
    // no esta descargado, se descarga. En una instalacion nueva el directorio de
    // modelos esta vacio, y sin esto la primera imagen que el usuario prueba falla
    // con «falta el modelo» — despues de haberla arrastrado y haber pulsado el
    // boton, que es el momento en el que menos se espera.
    //
    // El resultado no cancela el trabajo: si la descarga no sale (sin red, catalogo
    // sin URL) el lote se acepta igual. El motor tiene respaldo y el informe dice
    // con que se hizo; perder el lote entero por eso seria peor. Pero queda dicho
    // en el registro, que es donde se mira cuando el resultado sale interpolado.
    const ensured = await ensureModelsFor(
      client,
      {
        mode: request.mode,
        scale: request.scale,
        modelChainMode: request.options.modelChainMode,
        upscaleModel: request.options.upscaleModel,
      },
      (modelId) => models.download(modelId),
    )

    if (ensured.downloaded.length > 0 || ensured.unavailable.length > 0 || ensured.catalogError) {
      logger.info('models.ensured', {
        mode: request.mode,
        scale: request.scale,
        downloaded: ensured.downloaded,
        present: ensured.present,
        unavailable: ensured.unavailable,
        catalogError: ensured.catalogError,
        interpolated: ensured.interpolated,
      })
    }

    return client.createJob({ ...request, items: expandido.items })
  })

  handle<[], SidecarJob[]>(IPC.sidecarListJobs, () => client.listJobs())

  handle<[unknown], SidecarJob>(IPC.sidecarGetJob, (raw) =>
    client.getJob(requireId(raw, 'trabajo')),
  )

  handle<[unknown], SidecarJob>(IPC.sidecarPauseJob, (raw) =>
    client.pauseJob(requireId(raw, 'trabajo')),
  )

  handle<[unknown], SidecarJob>(IPC.sidecarResumeJob, (raw) =>
    client.resumeJob(requireId(raw, 'trabajo')),
  )

  handle<[unknown], SidecarJob>(IPC.sidecarCancelJob, (raw) =>
    client.cancelJob(requireId(raw, 'trabajo')),
  )

  handle<[], SidecarStatus>(IPC.sidecarRestart, async () => {
    await supervisor.stop()
    return supervisor.start()
  })

  // Descarga de modelos. El servicio se devuelve para que el arranque pueda
  // cancelar lo que quede en vuelo al cerrar la aplicación.
  handle<[unknown], ModelDownloadEvent>(IPC.modelDownload, (raw) =>
    models.download(requireId(raw, 'modelo')),
  )

  handle<[unknown], boolean>(IPC.modelDownloadCancel, (raw) =>
    models.cancel(requireId(raw, 'modelo')),
  )

  // Eventos y cambios de estado hacia la interfaz.
  client.subscribe((event) => {
    // El resultado que el motor acaba de escribir se autoriza aqui, en el unico
    // sitio por el que pasa la noticia: cuando la interfaz pida compararlo, la
    // imagen ya se puede servir. Se comprueba la forma de la ruta porque este dato
    // llega de otro proceso; el resto de la validacion la hizo el motor al escribir.
    if (event.type === 'itemCompleted' && !registerMediaPath(event.outPath)) {
      logger.warn('media.output-path-rejected', { itemId: event.itemId })
    }

    broadcast(IPC.sidecarEvent, event)
  })

  supervisor.onStatusChange((status) => {
    broadcast(IPC.sidecarStatusChanged, status)
  })

  logger.info('ipc.sidecar-registered')
  return models
}

function requireId(raw: unknown, what: string): string {
  if (typeof raw !== 'string' || raw.length === 0 || raw.length > 128) {
    throw new Error(`Identificador de ${what} no valido`)
  }
  return raw
}

/**
 * Mensaje de error para los archivos comprimidos que no se pudieron abrir.
 *
 * Nombra el archivo y el motivo de cada uno. Un "no se pudo abrir el archivo"
 * sin decir cual ni por que obliga al usuario a adivinar cual de los cincuenta
 * elementos del lote es el que sobra.
 */
function describirFallosDeArchivo(fallos: ArchiveFailure[]): string {
  const partes = fallos.map((fallo) => `${basename(fallo.item)}: ${fallo.message}`)
  const cuantos = fallos.length === 1 ? 'un archivo comprimido' : `${fallos.length} archivos comprimidos`
  return `No se pudo abrir ${cuantos}. ${partes.join('; ')}`
}

/**
 * Valida y normaliza una peticion de trabajo.
 *
 * Se construye un objeto nuevo con los campos comprobados en lugar de reenviar lo
 * que llego: asi, un campo de mas que enviara una version distinta de la interfaz
 * no se cuela hasta el sidecar.
 */
export function sanitizeJobRequest(raw: unknown): SidecarJobRequest {
  if (typeof raw !== 'object' || raw === null) {
    throw new Error('La peticion de trabajo no es un objeto')
  }

  const value = raw as Record<string, unknown>

  const mode = value['mode']
  if (mode !== 'photo' && mode !== 'illustration') {
    throw new Error(`Modo no valido: ${String(mode)}`)
  }

  const scale = value['scale']
  if (scale !== 2 && scale !== 4 && scale !== 8) {
    throw new Error(`Escala no valida: ${String(scale)}`)
  }

  const items = value['items']
  if (!Array.isArray(items) || items.length === 0) {
    throw new Error('El trabajo no tiene imagenes')
  }

  const paths: string[] = []
  for (const entry of items) {
    if (!isSafePathString(entry)) {
      throw new Error('Hay una ruta de archivo no valida en el trabajo')
    }
    paths.push(entry)
  }

  const output = value['output']
  if (typeof output !== 'object' || output === null) {
    throw new Error('Falta la configuracion de salida')
  }

  const outputRecord = output as Record<string, unknown>
  const dir = outputRecord['dir']
  if (!isSafePathString(dir)) {
    throw new Error('La carpeta de salida no es valida')
  }

  const format = outputRecord['format']
  if (format !== 'png' && format !== 'jpg' && format !== 'webp') {
    throw new Error(`Formato de salida no valido: ${String(format)}`)
  }

  const options = value['options']
  if (typeof options !== 'object' || options === null) {
    throw new Error('Faltan las opciones del trabajo')
  }
  const optionRecord = options as Record<string, unknown>

  return {
    mode,
    scale,
    items: paths,
    output: {
      dir,
      format,
      quality: clampNumber(outputRecord['quality'], 1, 100, 95),
      suffix: typeof outputRecord['suffix'] === 'string' ? outputRecord['suffix'].slice(0, 32) : '',
      preserveMetadata: outputRecord['preserveMetadata'] !== false,
      zipOutput: outputRecord['zipOutput'] === true,
    },
    options: {
      tileSize: normalizeTile(optionRecord['tileSize']),
      device:
        optionRecord['device'] === 'cpu' || optionRecord['device'] === 'gpu'
          ? optionRecord['device']
          : 'auto',
      concurrency: clampNumber(optionRecord['concurrency'], 1, 8, 1),
      unloadBetweenImages: optionRecord['unloadBetweenImages'] === true,
      modelChainMode: optionRecord['modelChainMode'] === 'manual' ? 'manual' : 'auto',
      upscaleModel:
        typeof optionRecord['upscaleModel'] === 'string' && optionRecord['upscaleModel'].length > 0
          ? optionRecord['upscaleModel']
          : null,
      faceRestore: normalizeFaceRestore(optionRecord['faceRestore']),
      denoise: normalizeDenoise(optionRecord['denoise']),
      sharpen: optionRecord['sharpen'] === true,
    },
    priority: clampNumber(value['priority'], 0, 9, 0),
  }
}

function clampNumber(raw: unknown, min: number, max: number, fallback: number): number {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) return fallback
  return Math.min(max, Math.max(min, Math.round(raw)))
}

function normalizeTile(raw: unknown): SidecarJobRequest['options']['tileSize'] {
  if (raw === 256 || raw === 384 || raw === 512 || raw === 768 || raw === 1024) return raw
  return 'auto'
}

function normalizeFaceRestore(raw: unknown): SidecarJobRequest['options']['faceRestore'] {
  if (raw === 'off' || raw === 'low' || raw === 'medium' || raw === 'high') return raw
  return 'auto'
}

function normalizeDenoise(raw: unknown): SidecarJobRequest['options']['denoise'] {
  if (raw === 'off' || raw === 'on') return raw
  return 'auto'
}
