import { app, dialog, shell } from 'electron'
import { access, mkdir, stat } from 'node:fs/promises'
import { constants } from 'node:fs'
import { join } from 'node:path'
import {
  IPC,
  MAX_DROP_FILES,
  MAX_STAT_FILES,
  isBoolean,
  isSafePathString,
  isString,
  toRuntimePlatform,
  toSafePathArray,
  type AppInfo,
  type AppSettings,
  type ExpandArchivesResult,
  type ExpandFolderResult,
  type FileStatResult,
  type ValidatePathsResult,
} from '@superupscaly/shared'
import { expandFolder, statFiles, validatePaths } from '../files/pathguard'
import { ArchiveService } from '../archives/service'
import { getLogDirectory, logger } from '../logging/logger'
import { registerMediaPath } from '../media/registry'
import { loadSettings, updateSettings } from '../store/settings'
import { handle } from './handle'

function defaultOutputDir(): string {
  return join(app.getPath('pictures'), 'Upscaled')
}

/** Cuanto se espera a que el sistema confirme que ha abierto una carpeta. */
const OPEN_PATH_TIMEOUT_MS = 1_500

/**
 * Abre una carpeta con el gestor de archivos del sistema.
 *
 * En Linux, `shell.openPath` **solo resuelve cuando el programa que lanza
 * termina**, y un gestor de archivos no termina: se queda abierto. Esperarlo deja
 * la llamada colgada indefinidamente —comprobado en este equipo: abrir `/tmp` no
 * resolvio en 60 segundos— y como esto vive dentro de un manejador de IPC, la
 * interfaz se queda con una promesa que nunca contesta y muestra un
 * "reply was never sent" que no dice nada de lo que ha pasado.
 *
 * Por eso se espera con limite. Que el limite se agote significa que el sistema ha
 * lanzado algo que sigue vivo, que es el caso normal; un fallo de verdad —una
 * carpeta que no existe— llega antes y se reporta como tal. Se registra cuando
 * vence el plazo para que la diferencia quede escrita y no supuesta.
 */
async function openInFileManager(target: string): Promise<boolean> {
  let timer: NodeJS.Timeout | undefined

  const expired = new Promise<boolean>((resolve) => {
    timer = setTimeout(() => {
      logger.debug('shell.open-path-sin-respuesta', { target })
      resolve(true)
    }, OPEN_PATH_TIMEOUT_MS)
  })

  try {
    const opened = shell.openPath(target).then((error) => error === '')
    return await Promise.race([opened, expired])
  } catch (error) {
    logger.warn('shell.open-path-failed', {
      target,
      message: error instanceof Error ? error.message : String(error),
    })
    return false
  } finally {
    // Sin esto, el temporizador seguiria vivo y dejaria en el registro un
    // "sin respuesta" de una llamada que si contesto.
    if (timer !== undefined) clearTimeout(timer)
  }
}

export function registerIpcHandlers(): void {
  handle<[unknown], ValidatePathsResult>(IPC.pathsValidate, async (rawPaths) => {
    return validatePaths(toSafePathArray(rawPaths, MAX_DROP_FILES))
  })

  handle<[], string[]>(IPC.filesPick, async () => {
    const result = await dialog.showOpenDialog({
      title: 'Seleccionar imagenes',
      properties: ['openFile', 'multiSelections', 'dontAddToRecent'],
      filters: [
        { name: 'Imagenes y archivos', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff', 'avif', 'zip', 'cbz'] },
        { name: 'Todos los archivos', extensions: ['*'] },
      ],
    })
    if (result.canceled) return []
    const validated = await validatePaths(result.filePaths)
    return validated.accepted
  })

  handle<[unknown], string | null>(IPC.folderPick, async (rawTitle) => {
    const result = await dialog.showOpenDialog({
      title: isString(rawTitle) && rawTitle.length <= 120 ? rawTitle : 'Seleccionar carpeta',
      properties: ['openDirectory', 'createDirectory', 'dontAddToRecent'],
    })
    const first = result.filePaths[0]
    return result.canceled || !first ? null : first
  })

  handle<[unknown, unknown], ExpandFolderResult>(IPC.folderExpand, async (rawDir, rawRecursive) => {
    if (!isSafePathString(rawDir)) return { paths: [], scanned: 0, truncated: false }
    return expandFolder(rawDir, isBoolean(rawRecursive) ? rawRecursive : true)
  })

  // Los ZIP y CBZ se expanden **al encolar**, no al crear el trabajo. Antes se
  // expandian dentro del manejador de trabajos, que es el unico sitio por el que
  // pasan las rutas que ve el motor: la cola mostraba el `.cbz` mientras el
  // trabajo hablaba de las paginas extraidas, asi que ningun evento del sidecar
  // encontraba a quien referirse y la lista se quedaba en «pendiente» para
  // siempre aunque el resumen dijera que todo habia ido bien. Expandiendo aqui,
  // cada pagina es un item con su ruta real y el emparejamiento por ruta de
  // origen vuelve a funcionar (ver ADR-037).
  const archives = new ArchiveService()

  handle<[unknown], ExpandArchivesResult>(IPC.archivesExpand, async (rawPaths) => {
    const requested = toSafePathArray(rawPaths, MAX_DROP_FILES)
    const outcome = await archives.expandAll(requested)

    // Se autorizan para poder enseñarse solo las paginas que acabamos de extraer
    // nosotros, en nuestro propio directorio temporal. Lo que el renderer ya
    // conocia (las imagenes que paso la validacion) no se toca: la lista de
    // medios sigue siendo algo que solo el proceso principal alimenta, y esta
    // llamada no puede autorizar una ruta arbitraria del disco (ADR-035).
    const known = new Set(requested)
    for (const path of outcome.items) {
      if (!known.has(path)) registerMediaPath(path)
    }

    if (outcome.failed.length > 0) {
      logger.warn('archives.expand-partial', {
        requested: requested.length,
        failed: outcome.failed.length,
        codes: outcome.failed.map((failure) => failure.errorCode),
      })
    }

    return {
      items: outcome.items,
      failed: outcome.failed.map((failure) => ({
        path: failure.item,
        errorCode: failure.errorCode,
        message: failure.message,
      })),
    }
  })

  handle<[], string>(IPC.outputDefault, () => defaultOutputDir())

  handle<[unknown], { ok: boolean; path: string }>(IPC.dirEnsure, async (rawDir) => {
    if (!isSafePathString(rawDir)) return { ok: false, path: '' }
    try {
      await mkdir(rawDir, { recursive: true })
      await access(rawDir, constants.W_OK)
      return { ok: true, path: rawDir }
    } catch (error) {
      logger.warn('dir.ensure-failed', { code: (error as NodeJS.ErrnoException).code ?? 'unknown' })
      return { ok: false, path: rawDir }
    }
  })

  handle<[unknown], FileStatResult[]>(IPC.filesStat, async (rawPaths) => {
    // El limite es el de una expansion, no el de una tanda: la lista que llega
    // aqui ya paso por la validacion y puede ser el contenido de una carpeta
    // entera (ver ADR-037).
    return statFiles(toSafePathArray(rawPaths, MAX_STAT_FILES))
  })

  handle<[], AppSettings>(IPC.settingsGet, () => loadSettings())

  handle<[unknown], AppSettings>(IPC.settingsSet, (patch) => updateSettings(patch))

  handle<[], AppInfo>(IPC.appInfo, () => ({
    name: app.getName(),
    version: app.getVersion(),
    electron: process.versions.electron ?? 'unknown',
    chrome: process.versions.chrome ?? 'unknown',
    node: process.versions.node ?? 'unknown',
    platform: toRuntimePlatform(process.platform),
    arch: process.arch,
    isPackaged: app.isPackaged,
  }))

  handle<[unknown], boolean>(IPC.reveal, async (rawPath) => {
    if (!isSafePathString(rawPath)) return false
    try {
      const info = await stat(rawPath)
      if (info.isDirectory()) {
        return await openInFileManager(rawPath)
      }
      shell.showItemInFolder(rawPath)
      return true
    } catch {
      return false
    }
  })

  handle<[], boolean>(IPC.openLogs, async () => {
    const dir = getLogDirectory()
    if (!dir) return false
    return openInFileManager(dir)
  })

  logger.info('ipc.registered')
}
