import { existsSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'

/**
 * Localizacion de ONNX Runtime para el sidecar.
 *
 * ## Por que lo resuelve el proceso principal
 *
 * El sidecar se compila contra `ort` con `load-dynamic`: la biblioteca de ONNX
 * Runtime no se enlaza, se carga en tiempo de ejecucion. `ort` la busca con
 * `dlopen("libonnxruntime.so")`, que **no** mira el directorio del ejecutable, asi
 * que sin ayuda encontraria la que tenga el sistema — o ninguna. La forma de
 * indicarle donde esta es `ORT_DYLIB_PATH`, y quien la conoce es el proceso
 * principal, que sabe donde vive el sidecar y donde estan los datos de la
 * aplicacion.
 *
 * ## Por que no se toca la variable si el usuario la ha definido
 *
 * `ORT_DYLIB_PATH` es la forma de probar un runtime distinto (una version con
 * CUDA, otra compilacion) sin recompilar nada. Si el usuario la define, manda.
 */

/** Nombres canonicos del nucleo de ONNX Runtime, por plataforma. */
export function runtimeLibraryNames(platform: NodeJS.Platform = process.platform): string[] {
  if (platform === 'win32') return ['onnxruntime.dll']
  if (platform === 'darwin') return ['libonnxruntime.dylib']
  return ['libonnxruntime.so']
}

export interface OrtOptions {
  /** Ruta absoluta al ejecutable del sidecar. */
  sidecarPath: string
  /** Directorio de datos de la aplicacion (`app.getPath('userData')`). */
  dataDir: string
  platform?: NodeJS.Platform
  /** Valor ya presente de `ORT_DYLIB_PATH`, si lo hay. */
  existing?: string | undefined
}

/**
 * Rutas candidatas, en orden de preferencia.
 *
 * 1. Lo que el usuario haya puesto en `ORT_DYLIB_PATH`.
 * 2. `<dataDir>/runtime/`: es donde la aplicacion puede instalar el runtime (y
 *    donde ira la version con CUDA cuando exista). Va antes que el directorio del
 *    binario porque sobrevive a recompilar el sidecar.
 * 3. Junto al sidecar: la convencion que ya documenta `docs/06` y la que usa el
 *    modo desarrollo, donde la biblioteca se copia al lado del binario compilado.
 */
export function ortLibraryCandidates(options: OrtOptions): string[] {
  const platform = options.platform ?? process.platform
  const names = runtimeLibraryNames(platform)

  const explicit = options.existing?.trim()
  if (explicit && explicit.length > 0) {
    return [explicit]
  }

  const directories = [join(options.dataDir, 'runtime'), dirname(options.sidecarPath)]

  return directories.flatMap((directory) => names.map((name) => join(directory, name)))
}

/** Raiz del nombre de la biblioteca del nucleo, sin extension ni version. */
function runtimeStem(name: string, platform: NodeJS.Platform): string | null {
  const lower = name.toLowerCase()

  if (lower.includes('providers')) {
    // Los execution providers no son el runtime.
    return null
  }

  const stems = platform === 'win32' ? ['onnxruntime'] : ['libonnxruntime', 'onnxruntime']
  const stem = stems.find((candidate) => lower.startsWith(candidate))

  if (!stem) return null

  const isLibrary =
    lower.endsWith('.so') || lower.endsWith('.dylib') || lower.endsWith('.dll') || lower.includes('.so.')

  return isLibrary ? stem : null
}

/**
 * Elige la biblioteca del nucleo entre los nombres de un directorio.
 *
 * Prefiere el nombre canonico (`libonnxruntime.so`) y, si no esta, el versionado
 * mas corto: en Linux es normal encontrarse `libonnxruntime.so.1.28.2` sin los
 * enlaces `libonnxruntime.so` y `.so.1`, y esa es una biblioteca perfectamente
 * valida. Lo que **no** vale es coger una de los execution providers: cargarla
 * como si fuera el motor falla con un mensaje que no explica nada.
 */
export function pickRuntimeLibrary(
  names: string[],
  platform: NodeJS.Platform = process.platform,
): string | null {
  const candidates: Array<{ exact: number; length: number; name: string }> = []

  for (const name of names) {
    const stem = runtimeStem(name, platform)
    if (!stem) continue

    const lower = name.toLowerCase()
    const canonical =
      lower === `${stem}.so` || lower === `${stem}.dylib` || lower === `${stem}.dll`

    candidates.push({ exact: canonical ? 0 : 1, length: name.length, name })
  }

  candidates.sort((left, right) => left.exact - right.exact || left.length - right.length)

  return candidates[0]?.name ?? null
}

/**
 * Ruta de la biblioteca de ONNX Runtime, o `null` si no hay ninguna.
 *
 * Devuelve un archivo, nunca un directorio: `ort` pasa el valor a `dlopen` tal
 * cual.
 */
export function resolveOrtLibrary(options: OrtOptions): string | null {
  const platform = options.platform ?? process.platform

  for (const candidate of ortLibraryCandidates(options)) {
    if (existsSync(candidate)) return candidate
  }

  // Un `ORT_DYLIB_PATH` explicito que no existe no se sustituye por otro: el
  // usuario pidio ese y merece un error claro, no un runtime distinto al que cree.
  if (options.existing && options.existing.trim().length > 0) return null

  // Ni el nombre canonico ni el versionado: se mira el contenido del directorio.
  const directories = [join(options.dataDir, 'runtime'), dirname(options.sidecarPath)]

  for (const directory of directories) {
    let entries: string[]
    try {
      entries = readdirSync(directory)
    } catch {
      // El directorio no existe: no es un error.
      continue
    }

    const chosen = pickRuntimeLibrary(entries, platform)
    if (chosen) return join(directory, chosen)
  }

  return null
}
