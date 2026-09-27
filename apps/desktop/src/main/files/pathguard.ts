import { constants } from 'node:fs'
import { access, readdir, realpath, stat } from 'node:fs/promises'
import { extname, isAbsolute, join } from 'node:path'
import {
  ALL_INPUT_EXTENSIONS,
  MAX_PATH_LENGTH,
  safePathRejection,
  type ExpandFolderResult,
  type FileStatResult,
  type RejectedPath,
  type ValidatePathsResult,
} from '@superupscaly/shared'
import { logger } from '../logging/logger'
import { registerMediaPath } from '../media/registry'

/** Limites defensivos: un drop accidental de una carpeta enorme no debe colgar la app. */
const MAX_EXPAND_FILES = 20_000
const MAX_EXPAND_DEPTH = 12

/** Por encima de esta longitud, Windows necesita el prefijo de ruta extendida. */
const WINDOWS_LONG_PATH_THRESHOLD = 240

function hasSupportedExtension(filePath: string): boolean {
  const ext = extname(filePath).slice(1).toLowerCase()
  return (ALL_INPUT_EXTENSIONS as readonly string[]).includes(ext)
}

/**
 * Normaliza una ruta para pasarla a un proceso hijo en Windows. Se aplica en el
 * momento de invocar al sidecar (Fase 2), no antes: la UI debe seguir mostrando
 * la ruta tal y como la escribio el usuario.
 */
export function toChildProcessPath(filePath: string): string {
  if (process.platform !== 'win32') return filePath
  if (filePath.startsWith('\\\\?\\')) return filePath
  if (filePath.length <= WINDOWS_LONG_PATH_THRESHOLD) return filePath
  if (filePath.startsWith('\\\\')) return `\\\\?\\UNC\\${filePath.slice(2)}`
  return `\\\\?\\${filePath}`
}

/** Expande una carpeta en la lista ordenada de imagenes compatibles que contiene. */
export async function expandFolder(dir: string, recursive = true): Promise<ExpandFolderResult> {
  const found: string[] = []
  let scanned = 0
  let truncated = false
  const queue: Array<{ dir: string; depth: number }> = [{ dir, depth: 0 }]

  while (queue.length > 0) {
    const current = queue.shift()
    if (!current) break

    let entries
    try {
      entries = await readdir(current.dir, { withFileTypes: true })
    } catch (error) {
      logger.warn('folder.read-failed', { code: (error as NodeJS.ErrnoException).code ?? 'unknown' })
      continue
    }

    for (const entry of entries) {
      if (found.length >= MAX_EXPAND_FILES) {
        truncated = true
        break
      }
      // Se ignoran los ocultos: en macOS y Linux suelen ser metadatos, no contenido.
      if (entry.name.startsWith('.')) continue

      const full = join(current.dir, entry.name)
      if (entry.isDirectory()) {
        if (recursive && current.depth < MAX_EXPAND_DEPTH) {
          queue.push({ dir: full, depth: current.depth + 1 })
        }
        continue
      }
      if (!entry.isFile()) continue

      scanned += 1
      if (hasSupportedExtension(full)) found.push(full)
    }

    if (truncated) break
  }

  found.sort((a, b) => a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' }))
  if (truncated) logger.warn('folder.truncated', { scanned, kept: found.length, limit: MAX_EXPAND_FILES })

  return { paths: found, scanned, truncated }
}

/**
 * Puerta de entrada de toda ruta que llega desde el renderer.
 *
 * Canonicaliza (resolviendo symlinks), comprueba legibilidad, descarta lo que no
 * sea un archivo soportado y expande las carpetas. Las rutas de la lista de
 * aceptadas son absolutas, reales y legibles; el resto se devuelve con un motivo
 * para que la UI pueda explicar que ha pasado.
 */
export async function validatePaths(input: readonly string[]): Promise<ValidatePathsResult> {
  const accepted: string[] = []
  const rejected: RejectedPath[] = []
  const seen = new Set<string>()

  const accept = (candidate: string): void => {
    if (seen.has(candidate)) return
    seen.add(candidate)
    accepted.push(candidate)
    // Aceptar una imagen y poder mostrarla son la misma decision: si se separan,
    // la cola aparece llena de imagenes que no se pueden previsualizar. Aqui la
    // ruta ya es absoluta, real, legible y de extension soportada — es justo la
    // unica lista blanca que necesita el servicio de imagenes del protocolo.
    registerMediaPath(candidate)
  }

  for (const raw of input) {
    const rejection = safePathRejection(raw)
    if (rejection) {
      rejected.push({ path: typeof raw === 'string' ? raw.slice(0, 200) : '', reason: rejection })
      continue
    }
    if (!isAbsolute(raw)) {
      rejected.push({ path: raw, reason: 'not-absolute' })
      continue
    }

    let resolved: string
    try {
      resolved = await realpath(raw)
    } catch {
      rejected.push({ path: raw, reason: 'not-found' })
      continue
    }

    if (resolved.length > MAX_PATH_LENGTH) {
      rejected.push({ path: resolved.slice(0, 200), reason: 'too-long' })
      continue
    }

    try {
      await access(resolved, constants.R_OK)
    } catch {
      rejected.push({ path: resolved, reason: 'not-readable' })
      continue
    }

    let info
    try {
      info = await stat(resolved)
    } catch {
      rejected.push({ path: resolved, reason: 'not-found' })
      continue
    }

    if (info.isDirectory()) {
      const expanded = await expandFolder(resolved, true)
      if (expanded.paths.length === 0) {
        rejected.push({ path: resolved, reason: 'empty-folder' })
        continue
      }
      for (const candidate of expanded.paths) accept(candidate)
      continue
    }

    if (!info.isFile()) {
      rejected.push({ path: resolved, reason: 'not-a-file' })
      continue
    }

    if (!hasSupportedExtension(resolved)) {
      rejected.push({ path: resolved, reason: 'unsupported-extension' })
      continue
    }

    accept(resolved)
  }

  logger.debug('paths.validated', { input: input.length, accepted: accepted.length, rejected: rejected.length })
  return { accepted, rejected }
}

/** Tamano y fecha de modificacion. Se usa para mostrar la cola sin leer las imagenes. */
export async function statFiles(paths: readonly string[]): Promise<FileStatResult[]> {
  const results: FileStatResult[] = []
  for (const candidate of paths) {
    if (safePathRejection(candidate) !== null) continue
    try {
      const info = await stat(candidate)
      results.push({
        path: candidate,
        sizeBytes: info.size,
        modifiedAt: Math.round(info.mtimeMs),
      })
    } catch {
      // Un archivo que desaparece entre la validacion y el stat simplemente no
      // aparece en el resultado; la UI lo tratara como fallo de lectura.
    }
  }
  return results
}
