/**
 * Validadores de la frontera IPC.
 *
 * Decision de la Fase 1: no se anade una libreria de esquemas (zod & cia.).
 * La superficie expuesta por el preload son ocho metodos con payloads triviales;
 * escribirlos a mano cuesta menos que versionar una dependencia mas en tres
 * plataformas. Se revisara en la Fase 4, cuando exista el cliente generado desde
 * el OpenAPI del sidecar y los esquemas se deriven de ahi.
 *
 * Regla: el proceso principal NUNCA confia en lo que llega del renderer.
 */

import {
  ALL_INPUT_EXTENSIONS,
  MAX_PATH_LENGTH,
  type AppSettings,
  type DeviceChoice,
  type DenoiseChoice,
  type FaceRestoreChoice,
  type Locale,
  type ModelChainMode,
  type OutputFormat,
  type ScaleFactor,
  type TileChoice,
  type AppMode,
} from './types.ts'

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function isString(value: unknown): value is string {
  return typeof value === 'string'
}

export function isBoolean(value: unknown): value is boolean {
  return typeof value === 'boolean'
}

export function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value)
}

export function isIntegerInRange(value: unknown, min: number, max: number): value is number {
  return isFiniteNumber(value) && Number.isInteger(value) && value >= min && value <= max
}

export function isOneOf<T extends string | number>(value: unknown, allowed: readonly T[]): value is T {
  return (allowed as readonly unknown[]).includes(value)
}

/**
 * Motivo por el que una cadena no es una ruta utilizable, o `null` si lo es.
 *
 * Se prefiere devolver el motivo a devolver un booleano porque el llamador casi
 * siempre necesita explicar al usuario que ha pasado, y porque un predicado de
 * tipo estrecharia el valor a `never` al negarlo (el parametro ya esta tipado
 * como `string`).
 */
export function safePathRejection(value: unknown): 'empty' | 'too-long' | null {
  if (typeof value !== 'string') return 'empty'
  if (value.length === 0) return 'empty'
  if (value.includes('\u0000')) return 'empty'
  if (value.length > MAX_PATH_LENGTH) return 'too-long'
  return null
}

/**
 * Comprueba que una cadena es una ruta que se puede pasar al sistema de
 * archivos sin sorpresas: sin bytes NUL, sin longitud absurda y no vacia.
 */
export function isSafePathString(value: unknown): value is string {
  return safePathRejection(value) === null
}

export function toSafePathArray(value: unknown, maxItems: number): string[] {
  if (!Array.isArray(value)) return []
  const out: string[] = []
  for (const entry of value) {
    if (out.length >= maxItems) break
    if (isSafePathString(entry)) out.push(entry)
  }
  return out
}

export function hasSupportedExtension(path: string): boolean {
  const dot = path.lastIndexOf('.')
  if (dot < 0 || dot === path.length - 1) return false
  const ext = path.slice(dot + 1).toLowerCase()
  return (ALL_INPUT_EXTENSIONS as readonly string[]).includes(ext)
}

const MODES: readonly AppMode[] = ['photo', 'illustration']
const SCALES: readonly ScaleFactor[] = [2, 4, 8]
const FORMATS: readonly OutputFormat[] = ['png', 'jpg', 'webp']
const LOCALES: readonly Locale[] = ['es', 'en']
const DEVICES: readonly DeviceChoice[] = ['auto', 'cpu', 'gpu']
const TILES: readonly TileChoice[] = ['auto', 256, 384, 512, 768, 1024]
const CHAIN_MODES: readonly ModelChainMode[] = ['auto', 'manual']
const DENOISE: readonly DenoiseChoice[] = ['off', 'auto', 'on']
const FACE: readonly FaceRestoreChoice[] = ['off', 'auto', 'low', 'medium', 'high']

/**
 * Normaliza un parche de ajustes. Se aplican **solo** los campos presentes y
 * validos; el resto se descarta en silencio. Devolver un objeto limpio en lugar
 * de lanzar mantiene la UI usable aunque una version antigua mande un campo que
 * ya no existe.
 */
export function sanitizeSettingsPatch(value: unknown): Partial<AppSettings> {
  if (!isPlainObject(value)) return {}
  const out: Partial<AppSettings> = {}

  if (isOneOf(value['locale'], LOCALES)) out.locale = value['locale']
  if (value['outputDir'] === null) out.outputDir = null
  else if (isSafePathString(value['outputDir'])) out.outputDir = value['outputDir']
  if (isOneOf(value['mode'], MODES)) out.mode = value['mode']
  if (isOneOf(value['scale'], SCALES)) out.scale = value['scale']
  if (isOneOf(value['outputFormat'], FORMATS)) out.outputFormat = value['outputFormat']
  if (isIntegerInRange(value['outputQuality'], 1, 100)) out.outputQuality = value['outputQuality']
  if (isString(value['suffix']) && value['suffix'].length <= 32) {
    out.suffix = value['suffix'].replace(/[\\/:*?"<>|\u0000]/g, '')
  }
  if (isBoolean(value['preserveMetadata'])) out.preserveMetadata = value['preserveMetadata']
  if (isBoolean(value['advancedOpen'])) out.advancedOpen = value['advancedOpen']
  if (isOneOf(value['tileSize'], TILES)) out.tileSize = value['tileSize']
  if (isOneOf(value['device'], DEVICES)) out.device = value['device']
  if (isIntegerInRange(value['concurrency'], 1, 8)) out.concurrency = value['concurrency']
  if (isBoolean(value['unloadBetweenImages'])) out.unloadBetweenImages = value['unloadBetweenImages']
  if (isOneOf(value['modelChainMode'], CHAIN_MODES)) out.modelChainMode = value['modelChainMode']
  if (isString(value['upscaleModel']) && value['upscaleModel'].length <= 64) {
    out.upscaleModel = value['upscaleModel']
  }
  if (isOneOf(value['faceRestore'], FACE)) out.faceRestore = value['faceRestore']
  if (isOneOf(value['denoise'], DENOISE)) out.denoise = value['denoise']
  if (isBoolean(value['sharpen'])) out.sharpen = value['sharpen']

  return out
}

export const DEFAULT_SETTINGS: AppSettings = {
  locale: 'es',
  outputDir: null,
  mode: 'photo',
  scale: 4,
  outputFormat: 'png',
  outputQuality: 95,
  suffix: '_upscaled',
  preserveMetadata: true,
  advancedOpen: false,
  tileSize: 'auto',
  device: 'auto',
  concurrency: 1,
  unloadBetweenImages: false,
  modelChainMode: 'auto',
  upscaleModel: '',
  faceRestore: 'auto',
  denoise: 'auto',
  sharpen: false,
}
