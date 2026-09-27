import { existsSync } from 'node:fs'
import { join } from 'node:path'

/**
 * Localizacion del binario del sidecar.
 *
 * La busqueda esta separada de la comprobacion en disco: `candidatePaths` es una
 * funcion pura y se puede testear sin tener el binario construido, que es
 * justamente el caso de cualquier maquina de desarrollo recien clonada.
 */

export interface SidecarLocation {
  /** Ruta absoluta al ejecutable. */
  command: string
  /** Argumentos previos al subcomando (`serve` y sus opciones los anade el supervisor). */
  baseArgs: string[]
  /** De donde salio, para el log. */
  origin: string
}

export interface LocatorOptions {
  isPackaged: boolean
  /** `process.resourcesPath` de Electron. */
  resourcesPath: string
  /** `app.getAppPath()`: en desarrollo, el directorio de `apps/desktop`. */
  appPath: string
  /** Plataforma y arquitectura, inyectables para poder testear. */
  platform?: NodeJS.Platform
  arch?: string
  /** Variable de entorno que fuerza una ruta concreta. */
  override?: string | undefined
}

/** Nombre del ejecutable segun la plataforma. */
export function binaryName(platform: NodeJS.Platform = process.platform): string {
  return platform === 'win32' ? 'su-cli.exe' : 'su-cli'
}

/**
 * Rutas candidatas, en orden de preferencia.
 *
 * En desarrollo se busca primero la build de release porque es la que el usuario
 * compila cuando quiere medir rendimiento; si solo existe la de debug, se usa
 * esa, que es lo que hay mientras se itera.
 */
export function candidatePaths(options: LocatorOptions): string[] {
  const platform = options.platform ?? process.platform
  const arch = options.arch ?? process.arch
  const name = binaryName(platform)

  const candidates: string[] = []

  if (options.override && options.override.length > 0) {
    candidates.push(options.override)
  }

  if (options.isPackaged) {
    // Empaquetado: el binario viaja junto a la aplicacion, por plataforma y
    // arquitectura, porque un instalador puede ser universal (macOS).
    candidates.push(join(options.resourcesPath, 'bin', `${platform}-${arch}`, name))
    candidates.push(join(options.resourcesPath, 'bin', name))
  } else {
    // Desarrollo: `appPath` es `apps/desktop`, asi que la raiz esta dos niveles
    // mas arriba.
    const root = join(options.appPath, '..', '..')
    const target = join(root, 'services', 'inference', 'target')

    candidates.push(join(target, 'release', name))
    candidates.push(join(target, 'debug', name))
  }

  return candidates
}

/**
 * Primer candidato que existe en disco.
 *
 * Devuelve `null` si no hay ninguno. No es un error en si mismo: significa que el
 * sidecar aun no se ha compilado, y la interfaz debe decirlo con claridad en
 * lugar de intentar arrancar algo que no esta.
 */
export function locateSidecar(options: LocatorOptions): SidecarLocation | null {
  const candidates = candidatePaths(options)

  for (const [index, candidate] of candidates.entries()) {
    if (existsSync(candidate)) {
      return {
        command: candidate,
        baseArgs: [],
        origin: index === 0 && options.override ? 'variable de entorno' : candidate,
      }
    }
  }

  return null
}

/**
 * Argumentos para arrancar el servidor.
 *
 * El token va por variable de entorno y no como argumento de linea de comandos:
 * los argumentos de un proceso son visibles para cualquier otro proceso del
 * equipo, y el token da acceso a la API local.
 */
/**
 * Argumentos para arrancar el servidor.
 *
 * `dataDir` es opcional y existe por un motivo concreto: el sidecar guarda ahi
 * los modelos, la base de datos de trabajos y el registro de calibracion. En
 * produccion se deja el valor por defecto del sidecar (el directorio de datos del
 * usuario); en desarrollo, poder apuntarlo a otra carpeta permite probar con un
 * catalogo de modelos distinto sin tocar los del usuario. Es un argumento global
 * de `clap`, asi que va **antes** del subcomando.
 */
export function serveArgs(portfile: string, port = 0, dataDir?: string): string[] {
  const args = ['serve', '--port', String(port), '--portfile', portfile]
  const override = dataDir?.trim()

  return override && override.length > 0 ? ['--data-dir', override, ...args] : args
}
