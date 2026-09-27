import { app } from 'electron'
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { DEFAULT_SETTINGS, sanitizeSettingsPatch, type AppSettings } from '@superupscaly/shared'
import { logger } from '../logging/logger'

let cache: AppSettings | null = null

function settingsPath(): string {
  return join(app.getPath('userData'), 'settings.json')
}

/**
 * Lectura tolerante: si el archivo no existe, esta corrupto o viene de una
 * version con campos distintos, se cae a los valores por defecto y se conserva
 * el archivo original como `.bak` para poder diagnosticarlo.
 */
export function loadSettings(): AppSettings {
  if (cache) return cache

  const file = settingsPath()
  let stored: Partial<AppSettings> = {}
  try {
    const raw = readFileSync(file, 'utf8')
    const parsed: unknown = JSON.parse(raw)
    stored = sanitizeSettingsPatch(parsed)
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code !== 'ENOENT') {
      logger.warn('settings.read-failed', { code: code ?? 'unknown' })
      try {
        renameSync(file, `${file}.bak`)
      } catch {
        // Si tampoco se puede renombrar, seguimos con los valores por defecto.
      }
    }
  }

  cache = { ...DEFAULT_SETTINGS, ...stored }
  return cache
}

/** Escritura atomica: archivo temporal + rename, nunca se deja un JSON a medias. */
export function saveSettings(settings: AppSettings): void {
  const file = settingsPath()
  const tmp = `${file}.tmp`
  try {
    mkdirSync(app.getPath('userData'), { recursive: true })
    writeFileSync(tmp, `${JSON.stringify(settings, null, 2)}\n`, 'utf8')
    renameSync(tmp, file)
  } catch (error) {
    logger.error('settings.write-failed', { code: (error as NodeJS.ErrnoException).code ?? 'unknown' })
  }
}

export function updateSettings(patch: unknown): AppSettings {
  const current = loadSettings()
  const clean = sanitizeSettingsPatch(patch)
  const next: AppSettings = { ...current, ...clean }
  cache = next
  saveSettings(next)
  logger.debug('settings.updated', { keys: Object.keys(clean) })
  return next
}
