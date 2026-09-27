/**
 * Contratos de dominio compartidos entre el renderer, el proceso principal y
 * (a partir de la Fase 2) el sidecar.
 *
 * Fase 1: estos tipos describen la UI y los ajustes. Los tipos de `Job`/`JobItem`
 * que devolvera el sidecar se anadiran en la Fase 2 y reutilizaran estos mismos
 * vocabularios (`ItemStatus`, `AppMode`, `ScaleFactor`, ...).
 */

export type AppMode = 'photo' | 'illustration'
export type ScaleFactor = 2 | 4 | 8
export type OutputFormat = 'png' | 'jpg' | 'webp'
export type Locale = 'es' | 'en'
export type DeviceChoice = 'auto' | 'cpu' | 'gpu'
export type TileChoice = 'auto' | 256 | 384 | 512 | 768 | 1024
export type ModelChainMode = 'auto' | 'manual'
export type DenoiseChoice = 'off' | 'auto' | 'on'
export type FaceRestoreChoice = 'off' | 'auto' | 'low' | 'medium' | 'high'

export type ItemStatus = 'pending' | 'running' | 'done' | 'degraded' | 'failed' | 'skipped'

/** Etapas del pipeline, en el orden en que las recorre la UI. */
export type StageId =
  | 'decode'
  | 'analyze'
  | 'denoise'
  | 'upscale'
  | 'face'
  | 'sharpen'
  | 'encode'

/** Item de la cola de trabajo tal como lo ve la UI. */
export interface QueueItem {
  /** ULID-ish local: solo identifica el item dentro de la sesion de UI. */
  id: string
  /** Ruta absoluta en disco. Nunca se envia al renderer desde otro origen. */
  path: string
  name: string
  dir: string
  sizeBytes: number
  status: ItemStatus
  /** 0..1 */
  progress: number
  stage?: StageId
  durationMs?: number
  errorCode?: string
  degraded?: boolean
  /**
   * Motivos por los que el resultado no es exactamente el que describe el
   * pipeline, tal cual los calcula el motor: etapas cuya condicion no se cumple,
   * etapas que aun no estan implementadas y modelos sustituidos por su
   * equivalente de reserva.
   *
   * El sidecar los envia desde el primer dia y la interfaz los tiraba: el usuario
   * tenia delante una imagen que no era la que el habia configurado, sin forma de
   * saber por que.
   */
  notes?: string[]
  /**
   * Archivo que escribio el motor para este item.
   *
   * Lo informa el evento `itemCompleted` y hasta ahora se descartaba: la interfaz
   * no tenia forma de saber donde habia quedado el resultado, asi que la unica
   * manera de mirarlo era salir a buscarlo al explorador de archivos.
   *
   * Se pierde al relanzar el lote (`normalizeForRun`), y es lo correcto: el
   * siguiente trabajo volvera a escribir el archivo y el anterior seria una
   * previsualizacion de algo que ya no es el resultado de esta ejecucion.
   */
  outPath?: string
}

/** Ajustes persistidos en `<appData>/settings.json`. */
export interface AppSettings {
  locale: Locale
  /** null = usar la carpeta por defecto del sistema (`~/Pictures/Upscaled`). */
  outputDir: string | null
  mode: AppMode
  scale: ScaleFactor
  outputFormat: OutputFormat
  /** Solo para jpg/webp. 1..100 */
  outputQuality: number
  suffix: string
  preserveMetadata: boolean
  advancedOpen: boolean
  tileSize: TileChoice
  device: DeviceChoice
  /** Items simultaneos. En la Fase 3 se documenta que 1 por GPU es el optimo. */
  concurrency: number
  unloadBetweenImages: boolean
  modelChainMode: ModelChainMode
  upscaleModel: string
  faceRestore: FaceRestoreChoice
  denoise: DenoiseChoice
  sharpen: boolean
}

export type PathRejectReason =
  | 'empty'
  | 'not-absolute'
  | 'not-found'
  | 'not-readable'
  | 'not-a-file'
  | 'too-long'
  | 'unsupported-extension'
  | 'empty-folder'

export interface RejectedPath {
  path: string
  reason: PathRejectReason
}

export interface ValidatePathsResult {
  accepted: string[]
  rejected: RejectedPath[]
}

export interface ExpandFolderResult {
  paths: string[]
  scanned: number
  truncated: boolean
}

/**
 * Plataformas soportadas.
 *
 * Se define aqui en lugar de usar `NodeJS.Platform` para que este paquete no
 * dependa de los tipos de Node: lo consume tambien el renderer, que no tiene
 * (ni debe tener) acceso a la API de Node.
 */
export type RuntimePlatform = 'win32' | 'darwin' | 'linux' | 'unknown'

export function toRuntimePlatform(value: string): RuntimePlatform {
  return value === 'win32' || value === 'darwin' || value === 'linux' ? value : 'unknown'
}

export interface AppInfo {
  name: string
  version: string
  electron: string
  chrome: string
  node: string
  platform: RuntimePlatform
  arch: string
  isPackaged: boolean
}

export interface FileStatResult {
  path: string
  sizeBytes: number
  modifiedAt: number
}

/** Extensiones aceptadas como entrada (debe coincidir con el brief funcional). */
export const IMAGE_EXTENSIONS = [
  'png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff', 'avif',
] as const

export const ARCHIVE_EXTENSIONS = ['zip', 'cbz'] as const

export const ALL_INPUT_EXTENSIONS = [...IMAGE_EXTENSIONS, ...ARCHIVE_EXTENSIONS] as const

export const SUPPORTED_SCALES: readonly ScaleFactor[] = [2, 4, 8] as const

export const MAX_PATH_LENGTH = 4096
export const MAX_DROP_FILES = 5000

/**
 * Limite de rutas en una consulta de tamanos.
 *
 * Es mas alto que [`MAX_DROP_FILES`] a proposito: una carpeta suelta entra como
 * **una** ruta en la cola y se expande en el proceso principal, asi que la lista
 * que se pregunta puede ser mucho mayor que lo que el usuario solto. Con el
 * limite de la cola, los archivos a partir del numero 5000 se quedaban sin
 * tamano y la lista los mostraba como `0 B` sin decir nada (ver ADR-037).
 *
 * Coincide con `MAX_EXPAND_FILES` de `main/files/pathguard.ts`, que es quien
 * decide cuantas rutas puede producir una expansion.
 */
export const MAX_STAT_FILES = 20_000

/** Prefijo de toda la superficie expuesta por el preload. */
export const PRELOAD_API_KEY = 'su' as const
