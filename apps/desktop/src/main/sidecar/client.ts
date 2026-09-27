import { WebSocket } from 'ws'
import type {
  SidecarCapabilities,
  SidecarEvent,
  SidecarJob,
  SidecarJobRequest,
  SidecarModelStatus,
  SidecarModelsResult,
  SidecarPipelinesResult,
} from '@superupscaly/shared'
import { logger } from '../logging/logger'
import type { SidecarSupervisor } from './supervisor'

/**
 * Cliente de la API del sidecar.
 *
 * ## Errores
 *
 * El sidecar responde a cualquier fallo con `{ code, message }`, donde `code` es
 * un codigo del catalogo (`SU-E130`, por ejemplo). Se conserva el codigo en lugar
 * de convertirlo en un mensaje: la interfaz tiene sus propias traducciones y
 * necesita el codigo para elegirlas. Un `Error` generico obligaria a analizar
 * texto, que es fragil.
 *
 * ## Eventos
 *
 * La suscripcion se reconecta sola. El sidecar puede reiniciarse mientras la
 * aplicacion sigue abierta, y una conexion rota sin reconexion dejaria la barra
 * de progreso congelada sin explicar nada.
 */

export class SidecarError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message)
    this.name = 'SidecarError'
  }
}

const REQUEST_TIMEOUT_MS = 15_000
const RECONNECT_BASE_MS = 500
const RECONNECT_MAX_MS = 10_000

export class SidecarClient {
  private socket: WebSocket | null = null
  private reconnectDelay = RECONNECT_BASE_MS
  private reconnectTimer: NodeJS.Timeout | null = null
  private closed = false
  private readonly listeners = new Set<(event: SidecarEvent) => void>()

  constructor(private readonly supervisor: SidecarSupervisor) {}

  /** `true` si el sidecar esta listo para atender peticiones. */
  isReady(): boolean {
    return this.supervisor.getStatus().state === 'ready' && this.supervisor.getBaseUrl() !== null
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const baseUrl = this.supervisor.getBaseUrl()
    if (!baseUrl) {
      throw new SidecarError('SU-E120', 'El sidecar no esta disponible.')
    }

    let response: Response
    try {
      response = await fetch(`${baseUrl}${path}`, {
        ...init,
        headers: {
          ...init.headers,
          Authorization: `Bearer ${this.supervisor.getToken()}`,
          'Content-Type': 'application/json',
        },
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      })
    } catch (error) {
      // Un fallo de red aqui casi siempre significa que el proceso murio entre
      // la comprobacion y la peticion.
      throw new SidecarError(
        'SU-E131',
        `No se pudo contactar con el sidecar: ${error instanceof Error ? error.message : String(error)}`,
      )
    }

    if (!response.ok) {
      throw await toSidecarError(response)
    }

    if (response.status === 204) {
      return undefined as T
    }

    return (await response.json()) as T
  }

  async capabilities(): Promise<SidecarCapabilities> {
    return this.request<SidecarCapabilities>('/v1/capabilities')
  }

  async models(): Promise<SidecarModelsResult> {
    const body = await this.request<{ modelsDir: string; models: SidecarModelStatus[] }>(
      '/v1/models',
    )
    // El sidecar resuelve dónde viven los modelos y la aplicación descarga ahí:
    // se conserva tal cual en lugar de recalcularlo, que es como las dos rutas
    // acabarían separándose.
    return { modelsDir: body.modelsDir, models: body.models }
  }

  /**
   * Los pipelines que el motor va a usar de verdad.
   *
   * Se piden al sidecar en lugar de reproducir sus reglas aquí: cuál es el modelo
   * de una etapa es una decisión del motor, y la aplicación solo necesita saberlo
   * para descargarlo antes de aceptar el trabajo.
   */
  async pipelines(): Promise<SidecarPipelinesResult> {
    return this.request<SidecarPipelinesResult>('/v1/pipelines')
  }

  async createJob(request: SidecarJobRequest): Promise<SidecarJob> {
    return this.request<SidecarJob>('/v1/jobs', {
      method: 'POST',
      body: JSON.stringify(request),
    })
  }

  async listJobs(): Promise<SidecarJob[]> {
    return this.request<SidecarJob[]>('/v1/jobs')
  }

  async getJob(id: string): Promise<SidecarJob> {
    return this.request<SidecarJob>(`/v1/jobs/${encodeURIComponent(id)}`)
  }

  async pauseJob(id: string): Promise<SidecarJob> {
    return this.request<SidecarJob>(`/v1/jobs/${encodeURIComponent(id)}/pause`, { method: 'POST' })
  }

  async resumeJob(id: string): Promise<SidecarJob> {
    return this.request<SidecarJob>(`/v1/jobs/${encodeURIComponent(id)}/resume`, { method: 'POST' })
  }

  async cancelJob(id: string): Promise<SidecarJob> {
    return this.request<SidecarJob>(`/v1/jobs/${encodeURIComponent(id)}/cancel`, { method: 'POST' })
  }

  /**
   * Se suscribe al flujo de eventos. Devuelve la funcion para darse de baja.
   *
   * La conexion se abre de forma perezosa: si no hay nadie escuchando, no hay
   * socket. Se cierra cuando se va el ultimo suscriptor.
   */
  subscribe(listener: (event: SidecarEvent) => void): () => void {
    this.listeners.add(listener)
    this.closed = false

    if (!this.socket) {
      this.openSocket()
    }

    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) {
        this.closeSocket()
      }
    }
  }

  private openSocket(): void {
    const baseUrl = this.supervisor.getBaseUrl()
    if (!baseUrl) {
      // Sin sidecar no hay nada a lo que conectarse; se reintenta mas tarde en
      // lugar de fallar, porque el sidecar puede estar arrancando.
      this.scheduleReconnect()
      return
    }

    const url = `${baseUrl.replace('http://', 'ws://')}/v1/events`

    try {
      // Se usa `ws` en lugar del `WebSocket` global de Node porque la API del
      // estandar no permite enviar cabeceras, y el token viaja en `Authorization`.
      // Pasarlo por la URL lo dejaria en los logs del servidor.
      this.socket = new WebSocket(url, {
        headers: { Authorization: `Bearer ${this.supervisor.getToken()}` },
      })
    } catch (error) {
      logger.warn('sidecar.ws-open-failed', {
        message: error instanceof Error ? error.message : String(error),
      })
      this.scheduleReconnect()
      return
    }

    this.socket.on('open', () => {
      logger.info('sidecar.events-connected')
      this.reconnectDelay = RECONNECT_BASE_MS
    })

    this.socket.on('message', (data: unknown) => {
      this.dispatch(data)
    })

    this.socket.on('close', () => {
      this.socket = null
      if (!this.closed) {
        this.scheduleReconnect()
      }
    })

    this.socket.on('error', (error: Error) => {
      // El error no cierra el socket por si solo; el cierre que le sigue es el
      // que dispara la reconexion, asi que aqui solo se registra.
      logger.warn('sidecar.ws-error', { message: error.message })
    })
  }

  private dispatch(raw: unknown): void {
    const text = typeof raw === 'string' ? raw : String(raw)

    let event: SidecarEvent
    try {
      event = JSON.parse(text) as SidecarEvent
    } catch {
      logger.warn('sidecar.event-unparsable')
      return
    }

    for (const listener of this.listeners) {
      try {
        listener(event)
      } catch (error) {
        // Un suscriptor que falla no puede tumbar a los demas ni cortar el
        // flujo de eventos.
        logger.warn('sidecar.listener-failed', {
          message: error instanceof Error ? error.message : String(error),
        })
      }
    }
  }

  private scheduleReconnect(): void {
    if (this.closed || this.reconnectTimer) return

    const wait = this.reconnectDelay
    this.reconnectDelay = Math.min(this.reconnectDelay * 2, RECONNECT_MAX_MS)

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      if (!this.closed && this.listeners.size > 0) {
        this.openSocket()
      }
    }, wait)
  }

  private closeSocket(): void {
    this.closed = true

    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

    if (this.socket) {
      try {
        this.socket.close()
      } catch {
        // Ya estaba cerrado.
      }
      this.socket = null
    }
  }

  /** Cierra la conexion y descarta a los suscriptores. */
  dispose(): void {
    this.listeners.clear()
    this.closeSocket()
  }
}

/** Convierte una respuesta de error del sidecar en una excepcion con codigo. */
async function toSidecarError(response: Response): Promise<SidecarError> {
  try {
    const body = (await response.json()) as { code?: string; message?: string }
    return new SidecarError(
      body.code ?? `SU-E${response.status}`,
      body.message ?? `El sidecar respondio ${response.status}`,
    )
  } catch {
    return new SidecarError(`SU-E${response.status}`, `El sidecar respondio ${response.status}`)
  }
}
