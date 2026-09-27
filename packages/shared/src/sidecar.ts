/**
 * Contratos de la API del sidecar (protocolo v1).
 *
 * Es el espejo en TypeScript de los tipos de Rust (`su-core` y `su-jobs`). Los
 * nombres de campo van en `camelCase` en ambos lados porque Rust serializa con
 * `#[serde(rename_all = "camelCase")]`.
 *
 * Regla: si se cambia un tipo aqui, hay que cambiarlo alli. El protocolo lleva
 * version y el proceso principal comprueba que coincide antes de usar la API, de
 * modo que un sidecar viejo se detecta al arrancar en lugar de fallar de forma
 * opaca a mitad de un lote.
 */

export const SIDECAR_PROTOCOL_VERSION = 1

/** Contenido de `<appData>/runtime.json`, escrito por el sidecar al arrancar. */
export interface RuntimeInfo {
  port: number
  pid: number
  version: string
  protocol: number
  startedAt: string
}

export type SidecarJobStatus =
  | 'queued'
  | 'running'
  | 'paused'
  | 'completed'
  | 'partial'
  | 'failed'
  | 'cancelled'

export type SidecarItemStatus = 'pending' | 'running' | 'done' | 'degraded' | 'failed' | 'skipped'

export type ProviderName = 'TensorRT' | 'CUDA' | 'DirectML' | 'CoreML' | 'CPU'

export interface ProviderAvailability {
  kind: ProviderName
  available: boolean
  /** Por que no esta disponible. Nunca se informa de un EP sin explicar el motivo. */
  reason?: string | null
}

export interface GpuInfo {
  name: string
  vendor: 'nvidia' | 'amd' | 'intel' | 'apple' | 'unknown'
  vramTotalMb: number
  vramFreeMb: number
  driverVersion?: string | null
}

export interface CpuInfo {
  brand: string
  physicalCores: number
  logicalCores: number
}

export interface SidecarCapabilities {
  cpu: CpuInfo
  gpus: GpuInfo[]
  ramTotalMb: number
  providers: ProviderAvailability[]
  /** Primer EP disponible por prioridad. Siempre existe: CPU. */
  recommended: ProviderName
  ortVersion?: string | null
  /**
   * Motor con el que se infiere de verdad en este arranque.
   *
   * `recommended` describe lo que la máquina permitiría; esto describe lo que se
   * ha podido cargar. Cuando no hay ONNX Runtime utilizable vale
   * `clasico-<filtro>`: interpolación, sin modelos. Son dos cosas distintas y
   * confundirlas es lo que hacía creer que había calidad de modelo donde solo hay
   * un interpolador.
   */
  engine: string
}

export type ModelState = 'installed' | 'hashMismatch' | 'unverified' | 'missing'

/**
 * Lo que hace falta para descargar un modelo.
 *
 * El sidecar describe; quien descarga es la aplicación. Así el sidecar no
 * necesita un cliente HTTP ni TLS, y la descarga puede usar el proxy del sistema
 * y el directorio de datos del usuario, que son cosas de la capa de aplicación.
 */
export interface SidecarModelDownload {
  /** Nombre canónico del archivo dentro de `SidecarModelsResult.modelsDir`. */
  fileName: string
  /** Espejos en orden de preferencia. Nunca vacío. */
  urls: string[]
  /** `sha256` en hexadecimal. Obligatorio: sin hash no se ofrece descarga. */
  sha256: string
  /** Tamaño esperado en bytes. `0` = el manifiesto no lo declara. */
  sizeBytes: number
}

export interface SidecarModelStatus {
  id: string
  name: string
  kind: 'photo' | 'illustration' | 'denoise' | 'face' | 'detector' | 'classifier'
  scale: number
  state: ModelState
  path: string
  sizeBytes: number
  licenseWarning?: string | null
  /**
   * Datos de descarga, o `null` si no hay nada que descargar.
   *
   * Es `null` cuando el modelo ya está instalado, cuando el manifiesto apunta a
   * un archivo local (`localPath`), y cuando no declara URL o hash. La interfaz
   * debe distinguir los tres casos por `state` y `licenseWarning` en lugar de
   * ofrecer un botón que no puede funcionar.
   */
  download?: SidecarModelDownload | null
}

/**
 * Una etapa de un pipeline, tal y como la publica el sidecar en `/v1/pipelines`.
 *
 * Solo se declaran los campos que la aplicación necesita para decidir **qué
 * modelo hay que tener descargado**: el identificador del modelo y la escala que
 * la etapa promete. Copiar el esquema entero aquí lo convertiría en una segunda
 * definición que se quedaría atrás.
 */
export interface SidecarPipelineStage {
  id: string
  op: 'analyze' | 'model' | 'resize' | 'unsharp' | 'denoise' | 'compose'
  model?: string
  fallbackModel?: string
  /** Escala que produce la etapa. Ausente significa «la del modelo». */
  scaleOut?: number
  /**
   * Condición que decide si la etapa entra.
   *
   * Se deja sin tipar a propósito: la interfaz no evalúa condiciones —eso lo hace
   * el motor con los números del análisis— y solo necesita saber **si hay una**.
   * Tiparla entera invitaría a reproducir el evaluador en el cliente, que es
   * justo el error que este campo ayuda a evitar.
   */
  when?: unknown
}

export interface SidecarPipeline {
  id: string
  mode: 'photo' | 'illustration'
  scale: number
  description?: string
  stages: SidecarPipelineStage[]
}

/** Respuesta de `/v1/pipelines`: los pipelines cargados, tal cual los lee el motor. */
export interface SidecarPipelinesResult {
  version: number
  pipelines: SidecarPipeline[]
}

/** Respuesta de `/v1/models`: el catálogo y dónde vive. */
export interface SidecarModelsResult {
  /**
   * Directorio donde el sidecar busca los modelos, tal cual lo resolvió él.
   *
   * La aplicación descarga aquí en vez de reproducir la lógica de plataforma que
   * decide la ruta: si las dos divergieran, la interfaz diría «instalado» y el
   * motor seguiría sin encontrar el archivo.
   */
  modelsDir: string
  models: SidecarModelStatus[]
}

export interface SidecarJobItem {
  id: string
  srcPath: string
  outPath?: string | null
  status: SidecarItemStatus
  attempt: number
  progress: number
  stage?: string | null
  durationMs?: number | null
  vramPeakMb?: number | null
  /** Etapas que se ejecutaron de verdad. Las condiciones pueden omitir algunas. */
  effectivePipeline: string[]
  /**
   * Etapas que no se ejecutaron y por qué, en formato `"etapa: motivo"`.
   *
   * `effectivePipeline` dice qué no se hizo; esto dice por qué. Se persiste con
   * el trabajo, así que sigue ahí al recargar la lista.
   */
  skipped: string[]
  errorCode?: string | null
  errorDetail?: string | null
  degraded: boolean
}

export interface SidecarJobProgress {
  total: number
  done: number
  failed: number
  degraded: number
}

export interface SidecarJob {
  id: string
  createdAt: string
  status: SidecarJobStatus
  mode: 'photo' | 'illustration'
  scale: number
  pipelineId: string
  progress: SidecarJobProgress
  items: SidecarJobItem[]
}

export interface SidecarJobRequest {
  mode: 'photo' | 'illustration'
  scale: number
  items: string[]
  output: {
    dir: string
    format: 'png' | 'jpg' | 'webp'
    quality: number
    suffix: string
    preserveMetadata: boolean
    zipOutput: boolean
  }
  options: {
    tileSize: 'auto' | 256 | 384 | 512 | 768 | 1024
    device: 'auto' | 'cpu' | 'gpu'
    concurrency: number
    unloadBetweenImages: boolean
    modelChainMode: 'auto' | 'manual'
    upscaleModel?: string | null
    faceRestore: 'off' | 'auto' | 'low' | 'medium' | 'high'
    denoise: 'off' | 'auto' | 'on'
    sharpen: boolean
  }
  priority: number
}

/** Sucesos del flujo WebSocket. `type` discrimina la union. */
export type SidecarEvent =
  | { type: 'jobCreated'; jobId: string; total: number }
  | { type: 'jobStarted'; jobId: string; total: number }
  | {
      type: 'jobProgress'
      jobId: string
      done: number
      failed: number
      degraded: number
      total: number
    }
  | { type: 'jobPaused'; jobId: string }
  | { type: 'jobResumed'; jobId: string }
  | {
      type: 'jobFinished'
      jobId: string
      status: SidecarJobStatus
      done: number
      failed: number
      degraded: number
    }
  | { type: 'itemStarted'; jobId: string; itemId: string; name: string }
  | {
      type: 'itemProgress'
      jobId: string
      itemId: string
      stage: string
      done: number
      total: number
      percent: number
    }
  | {
      type: 'itemCompleted'
      jobId: string
      itemId: string
      outPath: string
      durationMs: number
      executed: string[]
      skipped: string[]
      degraded: boolean
    }
  | { type: 'itemFailed'; jobId: string; itemId: string; code: string; detail: string }
  | { type: 'warning'; jobId: string | null; message: string }

/** Estado del sidecar tal como lo ve la interfaz. */
export type SidecarState =
  | 'stopped'
  | 'starting'
  | 'ready'
  | 'restarting'
  | 'failed'
  | 'unavailable'

export interface SidecarStatus {
  state: SidecarState
  /** Motivo cuando el estado no es `ready`. Se muestra al usuario. */
  detail?: string
  version?: string
  port?: number
  /** Intentos de reinicio realizados. */
  restarts: number
  /** true si el binario del sidecar no se encontro. */
  missingBinary: boolean
}

/** Limite de intentos de reinicio antes de rendirse y avisar. */
export const MAX_SIDECAR_RESTARTS = 5
