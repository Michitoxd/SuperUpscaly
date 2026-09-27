import type {
  AppInfo,
  AppSettings,
  ExpandFolderResult,
  FileStatResult,
  RuntimePlatform,
  ValidatePathsResult,
} from './types.ts'
import type {
  SidecarCapabilities,
  SidecarEvent,
  SidecarJob,
  SidecarJobRequest,
  SidecarModelStatus,
  SidecarModelsResult,
  SidecarPipelinesResult,
  SidecarStatus,
} from './sidecar.ts'
import type { ErrorCode } from './error-codes.ts'

/** Estado de una descarga de modelo. */
export type ModelDownloadStatus = 'started' | 'progress' | 'completed' | 'cancelled' | 'failed'

/**
 * Progreso de una descarga de modelo.
 *
 * Se emite muchas veces por segundo, así que lleva todo lo que la interfaz
 * necesita para pintar sin volver a preguntar nada: si faltara el total o la
 * velocidad, cada actualización provocaría otra llamada al proceso principal.
 */
export interface ModelDownloadEvent {
  modelId: string
  status: ModelDownloadStatus
  /** Bytes ya escritos en el archivo temporal. */
  receivedBytes: number
  /** Total esperado. `0` si el manifiesto no lo declara. */
  totalBytes: number
  /** `receivedBytes / totalBytes`, o `null` si no se conoce el total. */
  ratio: number | null
  /** Velocidad media desde que empezó, en bytes por segundo. */
  bytesPerSecond: number | null
  /** Espejo que se está usando. */
  url: string
  /** Código `SU-Exxx` cuando `status` es `'failed'`. */
  errorCode: ErrorCode | null
}

/**
 * Un archivo comprimido que no se pudo abrir al pasar por la cola.
 *
 * Lleva el archivo y el motivo: un lote que encola cincuenta imagenes y una que
 * no se puede abrir no puede quedarse sin decir cual ni por que.
 */
export interface ArchiveExpansionFailure {
  /** Ruta del archivo comprimido. */
  path: string
  errorCode: ErrorCode
  message: string
}

/**
 * Resultado de expandir los comprimidos de una lista de rutas.
 *
 * `items` respeta el orden de entrada y **puede crecer**: un CBZ de 200 paginas
 * ocupa 200 posiciones donde habia una sola. Un comprimido que no se pudo abrir
 * se queda en su sitio —con `failed` diciendo por que— para que el trabajo lo
 * reporte con su codigo y el usuario vea que ese archivo existe y no se proceso.
 */
export interface ExpandArchivesResult {
  items: string[]
  failed: ArchiveExpansionFailure[]
}

/**
 * Superficie que el preload expone en `window.su`.
 *
 * Reglas de diseno (ver `docs/02-arquitectura.md` §10):
 *  - Todo lo que cruza el puente es serializable (rutas, numeros, booleanos).
 *  - Los píxeles nunca cruzan: solo rutas.
 *  - Cada metodo valida sus argumentos en el proceso principal antes de tocar
 *    el sistema de archivos.
 */
export interface SuApi {
  /** Plataforma del renderer, para ajustes de UI sin ida y vuelta. */
  readonly platform: RuntimePlatform

  /**
   * Obtiene las rutas absolutas de una lista de `File` provenientes de un drop.
   * Se ejecuta en el preload con `webUtils.getPathForFile`, que es el unico
   * mecanismo valido desde Electron 32 (`File.path` fue eliminado).
   *
   * Devuelve `''` en las posiciones que no se pudieron resolver, de modo que el
   * llamador pueda avisar en lugar de fallar en silencio.
   */
  getPathsForFiles(files: File[]): string[]

  /**
   * Valida, canonicaliza y expande una lista de rutas. Las carpetas se expanden
   * de forma recursiva buscando imagenes compatibles.
   */
  validatePaths(paths: string[]): Promise<ValidatePathsResult>

  /** Abre el dialogo nativo de seleccion de archivos. */
  pickImages(): Promise<string[]>

  /** Abre el dialogo nativo de seleccion de carpeta. */
  pickFolder(title?: string): Promise<string | null>

  /** Expande una carpeta en la lista de imagenes que contiene. */
  expandFolder(dir: string, recursive?: boolean): Promise<ExpandFolderResult>

  /**
   * Sustituye cada ZIP o CBZ de la lista por las imagenes que contiene.
   *
   * La cola encola **paginas**, no archivos comprimidos. Es lo que hace que cada
   * item que el usuario ve tenga su propio progreso, su propio resultado y su
   * propia comparacion antes y despues: el motor trabaja con rutas de imagen, y
   * hasta ahora la lista mostraba el `.cbz` mientras el trabajo hablaba de las
   * paginas extraidas, asi que ningun evento encontraba a quien referirse
   * (ver ADR-037).
   */
  expandArchives(paths: string[]): Promise<ExpandArchivesResult>

  /** Carpeta de salida por defecto del sistema, p. ej. `~/Pictures/Upscaled`. */
  getDefaultOutputDir(): Promise<string>

  /** Comprueba que se puede escribir en una carpeta; la crea si no existe. */
  ensureDir(dir: string): Promise<{ ok: boolean; path: string }>

  /** Tamano y fecha de modificacion de una lista de archivos. */
  statFiles(paths: string[]): Promise<FileStatResult[]>

  getSettings(): Promise<AppSettings>
  setSettings(patch: Partial<AppSettings>): Promise<AppSettings>

  getAppInfo(): Promise<AppInfo>

  /** Muestra un archivo o carpeta en el gestor de archivos del sistema. */
  revealInFolder(path: string): Promise<boolean>

  /** Abre la carpeta de logs. */
  openLogsFolder(): Promise<boolean>

  // --- Sidecar -------------------------------------------------------------

  /** Estado del proceso sidecar: arrancando, listo, reiniciando o caido. */
  sidecarStatus(): Promise<SidecarStatus>

  /** Hardware detectado y execution providers disponibles. */
  sidecarCapabilities(): Promise<SidecarCapabilities>

  /** Catálogo de modelos y su estado en el cache local, más dónde vive. */
  sidecarModels(): Promise<SidecarModelsResult>
  /**
   * Los pipelines que el motor va a usar de verdad.
   *
   * El panel de ajustes avanzados dibuja lo que se va a ejecutar, y hasta ahora
   * lo hacia con una copia en cliente de las reglas del motor: nombraba el modelo
   * de 4x tambien en 2x, y dibujaba etapas que el analisis puede omitir. Preguntar
   * al que manda cuesta una llamada local y no se puede desincronizar.
   */
  sidecarPipelines(): Promise<SidecarPipelinesResult>

  /**
   * Descarga un modelo desde su origen hasta el directorio que indica el
   * sidecar, comprueba su `sha256` y lo deja en su sitio.
   *
   * Resuelve cuando la descarga termina, se cancela o falla: el evento devuelto
   * lleva el estado final. No rechaza, porque un fallo de red no es una
   * excepción del programa sino un resultado que hay que contar en la interfaz.
   */
  downloadModel(modelId: string): Promise<ModelDownloadEvent>

  /** Cancela una descarga en curso. `false` si ya había terminado. */
  cancelModelDownload(modelId: string): Promise<boolean>

  /**
   * Se suscribe al progreso de las descargas. Devuelve la función para darse de
   * baja: sin ella, cada cambio de pantalla dejaría un oyente vivo.
   */
  onModelDownload(listener: (event: ModelDownloadEvent) => void): () => void

  createJob(request: SidecarJobRequest): Promise<SidecarJob>
  listJobs(): Promise<SidecarJob[]>
  getJob(id: string): Promise<SidecarJob>
  pauseJob(id: string): Promise<SidecarJob>
  resumeJob(id: string): Promise<SidecarJob>
  cancelJob(id: string): Promise<SidecarJob>

  /** Detiene y vuelve a arrancar el sidecar. */
  restartSidecar(): Promise<SidecarStatus>

  /**
   * Se suscribe al flujo de eventos del sidecar. Devuelve la funcion para darse
   * de baja: sin ella, cada cambio de pantalla dejaria un oyente vivo.
   */
  onSidecarEvent(listener: (event: SidecarEvent) => void): () => void

  /** Se suscribe a los cambios de estado del proceso. */
  onSidecarStatus(listener: (status: SidecarStatus) => void): () => void
}

/** Nombres de los canales IPC. Centralizados para no repetir literales. */
export const IPC = {
  pathsValidate: 'su:paths:validate',
  filesPick: 'su:files:pick',
  folderPick: 'su:folder:pick',
  folderExpand: 'su:folder:expand',
  archivesExpand: 'su:archives:expand',
  outputDefault: 'su:output:default',
  dirEnsure: 'su:dir:ensure',
  filesStat: 'su:files:stat',
  settingsGet: 'su:settings:get',
  settingsSet: 'su:settings:set',
  appInfo: 'su:app:info',
  reveal: 'su:app:reveal',
  openLogs: 'su:app:open-logs',

  // Sidecar: peticiones
  sidecarStatus: 'su:sidecar:status',
  sidecarCapabilities: 'su:sidecar:capabilities',
  sidecarModels: 'su:sidecar:models',
  sidecarPipelines: 'su:sidecar:pipelines',
  sidecarCreateJob: 'su:sidecar:job:create',
  sidecarListJobs: 'su:sidecar:job:list',
  sidecarGetJob: 'su:sidecar:job:get',
  sidecarPauseJob: 'su:sidecar:job:pause',
  sidecarResumeJob: 'su:sidecar:job:resume',
  sidecarCancelJob: 'su:sidecar:job:cancel',
  sidecarRestart: 'su:sidecar:restart',

  // Descarga de modelos
  modelDownload: 'su:models:download',
  modelDownloadCancel: 'su:models:download-cancel',

  // Sidecar: notificaciones del proceso principal al renderer
  sidecarEvent: 'su:sidecar:event',
  sidecarStatusChanged: 'su:sidecar:status-changed',
  modelDownloadEvent: 'su:models:download-event',
} as const

export type IpcChannel = (typeof IPC)[keyof typeof IPC]
