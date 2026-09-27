const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  if (bytes < 1000) return `${Math.round(bytes)} B`
  let value = bytes
  let unit = 0
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  const decimals = value < 10 ? 1 : 0
  return `${value.toFixed(decimals)} ${BYTE_UNITS[unit] ?? 'B'}`
}

/** Duracion legible: `4 s`, `1 min 12 s`, `1 h 04 min`. */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—'
  const totalSeconds = Math.round(ms / 1000)
  if (totalSeconds < 60) return `${totalSeconds} s`
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  if (minutes < 60) return `${minutes} min ${String(seconds).padStart(2, '0')} s`
  const hours = Math.floor(minutes / 60)
  return `${hours} h ${String(minutes % 60).padStart(2, '0')} min`
}

export function formatPercent(value: number): string {
  return `${Math.round(Math.min(1, Math.max(0, value)) * 100)}%`
}

/**
 * Separadores de ruta de cualquiera de las tres plataformas. Se usa en lugar de
 * `node:path` porque este codigo corre en el renderer, sin acceso a Node.
 */
const SEPARATORS = /[\\/]+/

export function basename(filePath: string): string {
  const parts = filePath.split(SEPARATORS).filter(Boolean)
  return parts.length > 0 ? (parts[parts.length - 1] ?? filePath) : filePath
}

export function dirname(filePath: string): string {
  const index = Math.max(filePath.lastIndexOf('/'), filePath.lastIndexOf('\\'))
  return index > 0 ? filePath.slice(0, index) : filePath
}

export function extensionOf(filePath: string): string {
  const name = basename(filePath)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
}

/** Acorta una ruta larga por el centro: `C:\fotos\...\retrato.png`. */
export function shortenPath(filePath: string, maxLength = 46): string {
  if (filePath.length <= maxLength) return filePath
  const name = basename(filePath)
  const head = filePath.slice(0, Math.max(6, maxLength - name.length - 5))
  return `${head}…${name}`
}

/** Nombres de archivo duplicados reciben sufijos `(2)`, `(3)`… para no confundir al usuario. */
export function dedupeName(name: string, taken: ReadonlySet<string>): string {
  if (!taken.has(name)) return name
  const dot = name.lastIndexOf('.')
  const stem = dot > 0 ? name.slice(0, dot) : name
  const ext = dot > 0 ? name.slice(dot) : ''
  let counter = 2
  let candidate = `${stem} (${counter})${ext}`
  while (taken.has(candidate)) {
    counter += 1
    candidate = `${stem} (${counter})${ext}`
  }
  return candidate
}
