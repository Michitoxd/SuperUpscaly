import { spawn, type ChildProcess } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import {
  MAX_SIDECAR_RESTARTS,
  SIDECAR_PROTOCOL_VERSION,
  type RuntimeInfo,
  type SidecarStatus,
} from '@superupscaly/shared'
import { logger } from '../logging/logger'
import { locateSidecar, serveArgs, type LocatorOptions } from './locator'
import { resolveOrtLibrary } from './ort'

/**
 * Ciclo de vida del proceso sidecar.
 *
 * ## Por que existe
 *
 * El sidecar es un proceso aparte que puede morir: por un fallo de driver, por
 * falta de memoria o porque alguien lo mata. Sin un supervisor, la interfaz se
 * queda esperando respuestas que no llegan y el usuario no entiende por que.
 *
 * ## Decisiones
 *
 * - **El token va por entorno, nunca por argumentos.** Los argumentos de un
 *   proceso son visibles para cualquier otro proceso del equipo.
 * - **El portfile se valida contra el PID del hijo.** Un portfile de una sesion
 *   anterior se leeria al instante y apuntaria a un puerto muerto. Comparar el PID
 *   es mas fiable que fiarse de la marca de tiempo del archivo.
 * - **El reinicio usa backoff exponencial.** Si el sidecar muere al arrancar
 *   porque falta una biblioteca, reintentar en bucle solo llena el log. Se
 *   espacian los intentos y, tras unos cuantos, se rinde y se avisa.
 */

export interface SupervisorOptions extends LocatorOptions {
  /** Directorio de datos de la aplicacion (`app.getPath('userData')`). */
  dataDir: string
  /** Cuanto se espera a que el sidecar publique su puerto. */
  startupTimeoutMs?: number
}

export type StatusListener = (status: SidecarStatus) => void

const DEFAULT_STARTUP_TIMEOUT_MS = 10_000
const PORTFILE_POLL_MS = 150
const BACKOFF_BASE_MS = 1_000
const BACKOFF_MAX_MS = 30_000

export class SidecarSupervisor {
  private child: ChildProcess | null = null
  private token = ''
  private runtime: RuntimeInfo | null = null
  private location: ReturnType<typeof locateSidecar> = null
  private stopping = false
  private restarts = 0
  private backoffMs = BACKOFF_BASE_MS
  /** El hijo no llego a nacer (falta la biblioteca, no es ejecutable…). */
  private spawnFailed = false
  private restartTimer: NodeJS.Timeout | null = null
  private status: SidecarStatus = {
    state: 'stopped',
    restarts: 0,
    missingBinary: false,
  }
  private readonly listeners = new Set<StatusListener>()

  constructor(private readonly options: SupervisorOptions) {}

  getStatus(): SidecarStatus {
    return this.status
  }

  getRuntime(): RuntimeInfo | null {
    return this.runtime
  }

  /** Base de la API, o `null` si el sidecar no esta listo. */
  getBaseUrl(): string | null {
    return this.runtime ? `http://127.0.0.1:${this.runtime.port}` : null
  }

  getToken(): string {
    return this.token
  }

  onStatusChange(listener: StatusListener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private setStatus(patch: Partial<SidecarStatus>): void {
    this.status = { ...this.status, ...patch, restarts: this.restarts }
    for (const listener of this.listeners) {
      listener(this.status)
    }
  }

  /**
   * Arranca el sidecar y espera a que responda.
   *
   * Un `start()` explicito reinicia los contadores de reinicio. Es lo que
   * distingue «vuelve a intentarlo tu» de «vuelve a intentarlo solo»: los
   * arranques automaticos (`scheduleRestart`) se espacian con backoff y se rinden
   * a los `MAX_SIDECAR_RESTARTS`, pero si el usuario pulsa Reiniciar tras cinco
   * caidas, lo que pide es un intento limpio, no que la aplicacion siga contando
   * y se niegue a intentarlo.
   */
  async start(): Promise<SidecarStatus> {
    this.stopping = false
    this.restarts = 0
    this.backoffMs = BACKOFF_BASE_MS
    this.location = locateSidecar(this.options)

    if (!this.location) {
      logger.warn('sidecar.binary-not-found')
      this.setStatus({
        state: 'unavailable',
        missingBinary: true,
        detail:
          'No se encontro el ejecutable del sidecar. Compilalo con: cargo build --release -p su-cli',
      })
      return this.status
    }

    return this.launch()
  }

  private async launch(): Promise<SidecarStatus> {
    if (!this.location) {
      return this.status
    }

    this.spawnFailed = false

    this.setStatus({
      state: this.restarts > 0 ? 'restarting' : 'starting',
      missingBinary: false,
      detail: undefined,
    })

    // Token nuevo en cada arranque: si el proceso anterior filtro el suyo, deja
    // de servir.
    this.token = randomBytes(32).toString('hex')
    const portfile = join(this.options.dataDir, 'runtime.json')

    // ONNX Runtime no se enlaza: se carga en tiempo de ejecucion, y `ort` solo
    // mira `ORT_DYLIB_PATH` o el directorio de bibliotecas del sistema. Sin esta
    // variable el sidecar no encuentra el runtime que viaja con la aplicacion y
    // cae al motor clasico: peor resultado, sin ningun error que lo explique.
    const ortLibrary = resolveOrtLibrary({
      sidecarPath: this.location.command,
      dataDir: this.options.dataDir,
      existing: process.env.ORT_DYLIB_PATH,
    })

    if (ortLibrary) {
      logger.info('sidecar.ort-runtime', { library: ortLibrary })
    } else {
      // No es un error: el sidecar arranca igual y usa interpolacion clasica. Se
      // anota para que "¿por que este resultado es peor que el de la captura?"
      // tenga respuesta en el registro.
      logger.info('sidecar.ort-runtime-missing', {
        detail: 'sin ONNX Runtime: el sidecar usara interpolacion clasica',
      })
    }

    // Directorio de datos del sidecar. Por defecto es el del usuario, que es lo
    // correcto en produccion; `SU_DATA_DIR` permite usar otro catalogo de modelos
    // (desarrollo, pruebas, o dos instalaciones en la misma maquina).
    const sidecarDataDir = process.env.SU_DATA_DIR?.trim()

    if (sidecarDataDir && sidecarDataDir.length > 0) {
      logger.info('sidecar.data-dir-override', { dataDir: sidecarDataDir })
    }

    const child = spawn(
      this.location.command,
      [
        ...this.location.baseArgs,
        ...serveArgs(portfile, 0, sidecarDataDir && sidecarDataDir.length > 0 ? sidecarDataDir : undefined),
      ],
      {
        env: {
          ...process.env,
          SU_TOKEN: this.token,
          ...(ortLibrary ? { ORT_DYLIB_PATH: ortLibrary } : {}),
        },
        // La salida del sidecar se reenvia al log en lugar de a la consola: en
        // una aplicacion empaquetada no hay consola que mirar.
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
      },
    )

    this.child = child

    child.stdout?.on('data', (chunk: Buffer) => {
      logger.debug('sidecar.stdout', { line: chunk.toString().trim().slice(0, 400) })
    })
    child.stderr?.on('data', (chunk: Buffer) => {
      logger.warn('sidecar.stderr', { line: chunk.toString().trim().slice(0, 400) })
    })

    child.on('error', (error) => {
      logger.error('sidecar.spawn-failed', { message: error.message })
      // Se recuerda para que el bucle de espera no pise este motivo con el suyo:
      // «no respondio en 10 s» es peor explicacion que «no se pudo arrancar:
      // falta tal biblioteca», y es la que el usuario lee.
      this.spawnFailed = true
      this.setStatus({ state: 'failed', detail: `No se pudo arrancar: ${error.message}` })
    })

    child.on('exit', (code, signal) => {
      this.child = null
      this.runtime = null

      if (this.stopping) {
        logger.info('sidecar.stopped', { code, signal })
        this.setStatus({ state: 'stopped', port: undefined, detail: undefined })
        return
      }

      logger.warn('sidecar.exited-unexpectedly', { code, signal })
      this.scheduleRestart(code ?? -1)
    })

    const ready = await this.waitUntilReady(child, portfile)

    if (!ready) {
      return this.status
    }

    return this.status
  }

  /** Espera a que el sidecar publique el puerto y responda al health check. */
  private async waitUntilReady(child: ChildProcess, portfile: string): Promise<boolean> {
    const timeout = this.options.startupTimeoutMs ?? DEFAULT_STARTUP_TIMEOUT_MS
    const deadline = Date.now() + timeout

    while (Date.now() < deadline) {
      // El proceso no llego a existir: no hay nada que esperar y el motivo ya
      // esta escrito con mas detalle del que este bucle podria darle.
      if (this.spawnFailed) return false

      if (child.exitCode !== null) {
        this.setStatus({
          state: 'failed',
          detail: `El sidecar termino al arrancar (codigo ${child.exitCode}).`,
        })
        return false
      }

      const runtime = this.readPortfile(portfile, child.pid)

      if (runtime) {
        if (runtime.protocol !== SIDECAR_PROTOCOL_VERSION) {
          this.setStatus({
            state: 'failed',
            detail: `Version de protocolo incompatible: el sidecar habla v${runtime.protocol} y la aplicacion espera v${SIDECAR_PROTOCOL_VERSION}.`,
          })
          return false
        }

        if (await this.healthCheck(runtime.port)) {
          this.runtime = runtime
          this.backoffMs = BACKOFF_BASE_MS
          this.setStatus({
            state: 'ready',
            version: runtime.version,
            port: runtime.port,
            detail: undefined,
          })
          logger.info('sidecar.ready', { port: runtime.port, version: runtime.version })
          return true
        }
      }

      await delay(PORTFILE_POLL_MS)
    }

    this.setStatus({
      state: 'failed',
      detail: `El sidecar no respondio en ${Math.round(timeout / 1000)} s.`,
    })
    return false
  }

  /**
   * Lee el portfile solo si pertenece al proceso que acabamos de lanzar.
   *
   * Sin esta comprobacion, un `runtime.json` de una sesion anterior se leeria en
   * el primer intento y la aplicacion intentaria hablar con un puerto muerto.
   */
  private readPortfile(portfile: string, expectedPid: number | undefined): RuntimeInfo | null {
    try {
      const parsed: unknown = JSON.parse(readFileSync(portfile, 'utf8'))

      if (typeof parsed !== 'object' || parsed === null) return null

      const candidate = parsed as Partial<RuntimeInfo>

      if (typeof candidate.port !== 'number' || typeof candidate.pid !== 'number') {
        return null
      }
      if (expectedPid !== undefined && candidate.pid !== expectedPid) {
        return null
      }
      if (typeof candidate.protocol !== 'number' || typeof candidate.version !== 'string') {
        return null
      }

      return {
        port: candidate.port,
        pid: candidate.pid,
        version: candidate.version,
        protocol: candidate.protocol,
        startedAt: candidate.startedAt ?? '',
      }
    } catch {
      // Todavia no existe o esta a medio escribir. No es un error.
      return null
    }
  }

  private async healthCheck(port: number): Promise<boolean> {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/v1/health`, {
        signal: AbortSignal.timeout(2_000),
      })
      return response.ok
    } catch {
      return false
    }
  }

  private scheduleRestart(exitCode: number): void {
    if (this.restarts >= MAX_SIDECAR_RESTARTS) {
      this.setStatus({
        state: 'failed',
        detail: `El sidecar se ha caido ${this.restarts} veces. Se deja de reintentar.`,
      })
      logger.error('sidecar.giving-up', { restarts: this.restarts })
      return
    }

    this.restarts += 1
    const wait = this.backoffMs
    this.backoffMs = Math.min(this.backoffMs * 2, BACKOFF_MAX_MS)

    this.setStatus({
      state: 'restarting',
      detail: `El sidecar se cerro (codigo ${exitCode}). Reintentando en ${Math.round(wait / 1000)} s…`,
    })

    this.restartTimer = setTimeout(() => {
      this.restartTimer = null
      void this.launch()
    }, wait)
  }

  /**
   * Cierre ordenado: se pide al sidecar que se apague, se le da margen y solo
   * entonces se le envia una senal.
   *
   * Matarlo directamente dejaria el trabajo en curso sin marcar como
   * interrumpido, y la reanudacion no lo ofreceria.
   */
  async stop(): Promise<void> {
    this.stopping = true

    if (this.restartTimer) {
      clearTimeout(this.restartTimer)
      this.restartTimer = null
    }

    const child = this.child
    if (!child) {
      this.setStatus({ state: 'stopped' })
      return
    }

    const baseUrl = this.getBaseUrl()
    if (baseUrl) {
      try {
        await fetch(`${baseUrl}/v1/shutdown`, {
          method: 'POST',
          headers: { Authorization: `Bearer ${this.token}` },
          signal: AbortSignal.timeout(2_000),
        })
      } catch {
        // Si no responde al cierre ordenado, se sigue con la senal.
      }
    }

    if (await waitForExit(child, 3_000)) {
      return
    }

    logger.warn('sidecar.forcing-termination')
    child.kill('SIGTERM')

    if (await waitForExit(child, 2_000)) {
      return
    }

    child.kill('SIGKILL')
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms)
  })
}

/** Espera a que el proceso termine. Devuelve `true` si lo hizo a tiempo. */
function waitForExit(child: ChildProcess, timeoutMs: number): Promise<boolean> {
  if (child.exitCode !== null) return Promise.resolve(true)

  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      child.removeListener('exit', onExit)
      resolve(false)
    }, timeoutMs)

    const onExit = (): void => {
      clearTimeout(timer)
      resolve(true)
    }

    child.once('exit', onExit)
  })
}
