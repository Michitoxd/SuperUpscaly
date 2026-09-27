import { app } from 'electron'
import { createWriteStream, mkdirSync, type WriteStream } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

export type LogLevel = 'debug' | 'info' | 'warn' | 'error'

const LEVEL_ORDER: Record<LogLevel, number> = { debug: 10, info: 20, warn: 30, error: 40 }

let stream: WriteStream | null = null
let logDirectory = ''
let minLevel: LogLevel = 'info'

const home = homedir()

/**
 * Enmascara el directorio personal. Los logs se comparten en informes de error,
 * asi que no deben revelar la estructura de carpetas del usuario.
 */
function mask(value: string): string {
  if (!home) return value
  return value.split(home).join('<home>')
}

export function initLogger(): string {
  logDirectory = join(app.getPath('userData'), 'logs')
  mkdirSync(logDirectory, { recursive: true })
  const stamp = new Date().toISOString().slice(0, 10)
  stream = createWriteStream(join(logDirectory, `superupscaly-${stamp}.log`), { flags: 'a' })
  minLevel = app.isPackaged ? 'info' : 'debug'
  log('info', 'logger.ready', { dir: logDirectory, level: minLevel })
  return logDirectory
}

export function getLogDirectory(): string {
  return logDirectory
}

export function log(level: LogLevel, message: string, fields?: Record<string, unknown>): void {
  if (LEVEL_ORDER[level] < LEVEL_ORDER[minLevel]) return

  const entry: Record<string, unknown> = {
    ts: new Date().toISOString(),
    level,
    msg: message,
    ...fields,
  }

  try {
    stream?.write(`${JSON.stringify(entry, (_key, value) => (typeof value === 'string' ? mask(value) : value))}\n`)
  } catch {
    // Un fallo al escribir el log no puede tumbar la aplicacion.
  }

  if (!app.isPackaged) {
    const printer = level === 'error' ? console.error : level === 'warn' ? console.warn : console.log
    printer(`[${level}] ${message}`, fields ?? '')
  }
}

export const logger = {
  debug: (message: string, fields?: Record<string, unknown>) => log('debug', message, fields),
  info: (message: string, fields?: Record<string, unknown>) => log('info', message, fields),
  warn: (message: string, fields?: Record<string, unknown>) => log('warn', message, fields),
  error: (message: string, fields?: Record<string, unknown>) => log('error', message, fields),
}
