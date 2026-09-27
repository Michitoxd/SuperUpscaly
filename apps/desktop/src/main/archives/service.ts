/**
 * Expansion de archivos ZIP y CBZ dentro de una peticion de trabajo.
 *
 * El sidecar recibe **rutas de imagen**, no archivos comprimidos. Este servicio
 * es el que traduce: si un elemento del trabajo es un ZIP o un CBZ, lo extrae a
 * un directorio temporal y devuelve las rutas de las imagenes que contiene, en
 * su orden.
 *
 * ## Por que en la aplicacion y no en el sidecar
 *
 * Mismo motivo que la descarga de modelos (ADR-019): es una tarea de sistema de
 * archivos que la aplicacion ya sabe hacer sin dependencias nuevas, y el sidecar
 * se queda con lo que solo el puede hacer. Ademas, asi el usuario ve el fallo
 * antes de encolar el trabajo, en vez de descubrirlo a mitad del lote.
 *
 * ## El directorio de extraccion se reutiliza
 *
 * La carpeta se deriva del `sha256` de la ruta del archivo, asi que volver a
 * lanzar el mismo CBZ no vuelve a extraerlo ni deja copias. **No se borra al
 * salir a proposito**: un trabajo en pausa guarda las rutas de las imagenes ya
 * extraidas, y borrarlas romperia la reanudacion entre sesiones. Es material
 * temporal y el propio sistema lo limpia.
 */

import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { extname, join } from 'node:path'

import { ARCHIVE_EXTENSIONS, IMAGE_EXTENSIONS, type ErrorCode } from '@superupscaly/shared'

// La extension es obligatoria para que este modulo se pueda cargar desde una
// prueba con `node --test` (ver ADR-036), no una cuestion de estilo.
import { expandArchive } from './reader.ts'

/** Un archivo comprimido que no se pudo abrir, con el motivo. */
export interface ArchiveFailure {
  item: string
  errorCode: ErrorCode
  message: string
}

export interface ExpandOutcome {
  /** Rutas de imagen: las de antes mas las extraidas. */
  items: string[]
  failed: ArchiveFailure[]
}

function esArchivoComprimido(path: string): boolean {
  const ext = extname(path).replace(/^\./, '').toLowerCase()
  return (ARCHIVE_EXTENSIONS as readonly string[]).includes(ext)
}

/** Directorio estable por archivo: el mismo CBZ va siempre al mismo sitio. */
function directorioDe(path: string, root: string): string {
  const huella = createHash('sha256').update(path).digest('hex').slice(0, 16)
  return join(root, huella)
}

export class ArchiveService {
  private readonly root: string

  constructor(root: string = join(tmpdir(), 'superupscaly-archives')) {
    this.root = root
  }

  isArchive(path: string): boolean {
    return esArchivoComprimido(path)
  }

  /**
   * Sustituye cada archivo comprimido por las imagenes que contiene.
   *
   * Los elementos que ya son imagenes pasan tal cual y **conservan su posicion
   * relativa**: el orden del lote es el que eligio el usuario, y las paginas de
   * un CBZ entran donde estaba el CBZ.
   *
   * Los fallos se devuelven en vez de lanzarse, para que quien llama decida. No
   * se descarta ninguno en silencio: un archivo que el usuario pidio y que no
   * aparece en el resultado seria un fallo mudo.
   *
   * Un comprimido que no se pudo abrir **se queda en la lista, en su sitio**, y
   * ademas aparece en `failed`. Las dos cosas son necesarias: la interfaz necesita
   * una ruta que enseñar en la fila (si desapareciera, la lista cambiaria de
   * tamano bajo los dedos del usuario) y quien llama necesita el motivo para
   * contarlo. La lista resultante nunca es mas corta que la de entrada.
   */
  async expandAll(items: string[]): Promise<ExpandOutcome> {
    const salida: string[] = []
    const failed: ArchiveFailure[] = []

    for (const item of items) {
      if (!this.isArchive(item)) {
        salida.push(item)
        continue
      }

      const resultado = await expandArchive({
        archivePath: item,
        destDir: directorioDe(item, this.root),
        imageExtensions: IMAGE_EXTENSIONS,
      })

      if (resultado.status === 'failed') {
        failed.push({ item, errorCode: resultado.errorCode, message: resultado.message })
        salida.push(item)
        continue
      }

      for (const imagen of resultado.images) salida.push(imagen.path)
    }

    return { items: salida, failed }
  }
}
