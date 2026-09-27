import {
  MAX_DROP_FILES,
  type ArchiveExpansionFailure,
  type QueueItem,
  type RejectedPath,
} from '@superupscaly/shared'
import { getBridge } from './bridge'
import { basename, dedupeName, dirname } from './format'

let idCounter = 0

function nextId(): string {
  idCounter += 1
  return `${Date.now().toString(36)}-${idCounter.toString(36)}`
}

export interface IngestResult {
  items: QueueItem[]
  rejected: RejectedPath[]
  /** Archivos soltados cuya ruta no se pudo resolver (Wayland, sandbox…). */
  unresolved: number
  /** Se ignoraron por superar el limite de la tanda. */
  overLimit: number
  /**
   * Comprimidos que no se pudieron abrir, con su motivo.
   *
   * El archivo **sigue en la cola** en su sitio: se cuenta lo que pasa, pero no
   * se esconde la fila. Al pulsar Upscaly el trabajo lo rechaza con el mismo
   * mensaje, que es donde el usuario espera la explicacion completa.
   */
  archivesFailed: ArchiveExpansionFailure[]
  /** true si la pagina no corre dentro de Electron. */
  noBridge: boolean
}

/**
 * Convierte los `File` de un drop en rutas absolutas.
 *
 * Se separa del resto del pipeline porque es lo unico que debe ocurrir de forma
 * sincrona dentro del manejador del evento `drop`: en cuanto el handler
 * devuelve, el `DataTransfer` se invalida y las rutas se pierden.
 */
export function resolveDroppedPaths(files: File[]): { paths: string[]; unresolved: number; overLimit: number } {
  const bridge = getBridge()
  if (!bridge) return { paths: [], unresolved: files.length, overLimit: 0 }

  const limited = files.slice(0, MAX_DROP_FILES)
  const raw = bridge.getPathsForFiles(limited)

  const paths: string[] = []
  // Los que sobran del limite de la tanda NO son rutas irresolubles: se cuentan
  // en `overLimit` y nada mas. Sumarlos aqui hacia que soltar 6000 archivos
  // dijera «6000 rutas no se pudieron resolver», que manda al usuario a buscar
  // un problema de portal Wayland que no tiene.
  let unresolved = 0

  for (const candidate of raw) {
    if (typeof candidate === 'string' && candidate.length > 0) paths.push(candidate)
    else unresolved += 1
  }

  return { paths, unresolved, overLimit: files.length - limited.length }
}

/**
 * Valida, expande y convierte rutas en items de cola.
 *
 * `validatePaths` hace el trabajo pesado en el proceso principal: canonicaliza,
 * comprueba permisos, filtra extensiones y expande carpetas. `expandArchives`
 * remata lo mismo con los comprimidos: un item de la cola es siempre **una
 * imagen**, nunca un `.zip` ni un `.cbz`, porque es lo unico que el motor sabe
 * procesar y lo unico que los eventos del sidecar pueden referenciar.
 */
export async function ingestPaths(rawPaths: readonly string[]): Promise<IngestResult> {
  const bridge = getBridge()
  if (!bridge) {
    return {
      items: [],
      rejected: [],
      unresolved: rawPaths.length,
      overLimit: 0,
      archivesFailed: [],
      noBridge: true,
    }
  }

  const validated = await bridge.validatePaths([...rawPaths])

  // Los ZIP y CBZ se cambian por sus paginas **antes** de encolar. El motor
  // trabaja con rutas de imagen, asi que si la cola guardara el `.cbz` no habria
  // forma de emparejar un solo evento: el progreso, el resultado y la comparacion
  // son por pagina, y la pagina es el item (ver ADR-037).
  const expanded = await bridge.expandArchives(validated.accepted)
  const stats = await bridge.statFiles(expanded.items)
  const sizeByPath = new Map(stats.map((entry) => [entry.path, entry.sizeBytes]))

  const items: QueueItem[] = expanded.items.map((filePath) => ({
    id: nextId(),
    path: filePath,
    name: basename(filePath),
    dir: dirname(filePath),
    sizeBytes: sizeByPath.get(filePath) ?? 0,
    status: 'pending',
    progress: 0,
  }))

  return {
    items,
    rejected: validated.rejected,
    unresolved: 0,
    overLimit: 0,
    archivesFailed: expanded.failed,
    noBridge: false,
  }
}

/**
 * Une los items nuevos a la cola evitando duplicados por ruta y renombrando las
 * coincidencias de nombre visible para que la lista no muestre dos veces
 * `foto.png` sin explicar cual es cual.
 */
export function mergeIntoQueue(current: readonly QueueItem[], incoming: readonly QueueItem[]): QueueItem[] {
  const existingPaths = new Set(current.map((item) => item.path))
  const takenNames = new Set(current.map((item) => item.name))
  const merged = [...current]

  for (const item of incoming) {
    if (existingPaths.has(item.path)) continue
    existingPaths.add(item.path)
    const uniqueName = dedupeName(item.name, takenNames)
    takenNames.add(uniqueName)
    merged.push(uniqueName === item.name ? item : { ...item, name: uniqueName })
  }

  return merged
}
