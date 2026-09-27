/**
 * Lector de archivos ZIP y CBZ.
 *
 * ## Por que existe
 *
 * La interfaz anuncia ZIP y CBZ como entrada: estan en `ALL_INPUT_EXTENSIONS`,
 * el selector de archivos los acepta y la zona de arrastre lo dice. Un CBZ es un
 * ZIP con imagenes dentro, y el formato es lo bastante simple como para leerlo
 * sin anadir una dependencia: Node ya trae el inflate y el CRC que hacen falta
 * (`node:zlib`), y el resto es recorrer cabeceras.
 *
 * ## Por que se lee el directorio central y no las cabeceras locales
 *
 * Un ZIP puede escribirse en streaming. En ese caso las cabeceras locales llevan
 * el tamano y el CRC a cero y los valores reales van en un descriptor detras de
 * los datos (bit 3 del campo de flags). El directorio central, en cambio, siempre
 * tiene los valores definitivos. Por eso se busca el EOCD al final, se lee el
 * directorio central, y solo se vuelve a la cabecera local para calcular donde
 * empiezan los datos.
 *
 * ## Lo que no se soporta, y por que se dice en vez de intentarlo
 *
 * ZIP64, entradas cifradas y compresion distinta de `stored` o `deflate`. Los
 * tres se detectan y se devuelven como error con su motivo. Leer un archivo a
 * medias produce una imagen corrupta que se ve rara y se diagnostica fatal; un
 * error que dice que pasa se arregla en un minuto.
 *
 * El modulo es **puro** a proposito: solo `node:*` e `import type`. Asi se puede
 * probar con `node --test` sin empaquetar nada (ver `reader.test.ts`).
 */

import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { basename, extname, join } from 'node:path'
import { crc32, inflateRawSync } from 'node:zlib'

import type { ErrorCode } from '@superupscaly/shared'

/** Firma de "fin del directorio central" (bytes `50 4B 05 06`). */
const EOCD = 0x06054b50
/** Firma de una entrada del directorio central (`50 4B 01 02`). */
const CENTRAL = 0x02014b50
/** Firma de una cabecera local de archivo (`50 4B 03 04`). */
const LOCAL = 0x04034b50

/** Tamano del EOCD sin contar el comentario final. */
const EOCD_MIN = 22
/** El comentario final puede llegar a 65535 bytes, asi que el EOCD puede estar
 *  hasta ese margen antes del final del archivo. */
const MAX_COMENTARIO = 0xffff

/** Metodos de compresion soportados. */
const ALMACENADO = 0
const DEFLATE = 8

/** Bit 0 de los flags: la entrada va cifrada. */
const FLAG_CIFRADO = 0x0001

/** Valor centinela que anuncia un campo de 64 bits en el extra field. */
const ZIP64 = 0xffffffff
const ZIP64_ENTRADAS = 0xffff

/** Una entrada del directorio central, ya interpretada. */
interface EntradaCentral {
  name: string
  method: number
  flags: number
  crc: number
  compressedSize: number
  uncompressedSize: number
  localOffset: number
}

/** Un fallo, con el codigo de la taxonomia comun. */
interface Fallo {
  status: 'failed'
  errorCode: ErrorCode
  message: string
}

function fallo(message: string, errorCode: ErrorCode = 'SU-E102'): Fallo {
  return { status: 'failed', errorCode, message }
}

function mensajeDe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** Busca el EOCD desde el final, que es donde tiene que estar. */
function localizarEocd(buf: Buffer): number | Fallo {
  if (buf.length < EOCD_MIN) {
    return fallo('el archivo es demasiado pequeno para ser un ZIP')
  }

  const desde = Math.max(0, buf.length - EOCD_MIN - MAX_COMENTARIO)
  for (let i = buf.length - EOCD_MIN; i >= desde; i--) {
    if (buf.readUInt32LE(i) === EOCD) return i
  }

  return fallo('no se encontro el final del directorio central: el archivo no es un ZIP o esta truncado')
}

/** Lee el directorio central entero. */
function leerDirectorioCentral(buf: Buffer, eocd: number): EntradaCentral[] | Fallo {
  const total = buf.readUInt16LE(eocd + 10)
  const offset = buf.readUInt32LE(eocd + 16)

  if (total === ZIP64_ENTRADAS || offset === ZIP64) {
    return fallo('el archivo usa ZIP64, que no se soporta')
  }

  const entradas: EntradaCentral[] = []
  let cursor = offset

  for (let i = 0; i < total; i++) {
    if (cursor + 46 > buf.length) {
      return fallo('el directorio central esta truncado')
    }
    if (buf.readUInt32LE(cursor) !== CENTRAL) {
      return fallo(`la entrada ${i + 1} del directorio central tiene una firma invalida`)
    }

    const flags = buf.readUInt16LE(cursor + 8)
    const method = buf.readUInt16LE(cursor + 10)
    const crc = buf.readUInt32LE(cursor + 16)
    const compressedSize = buf.readUInt32LE(cursor + 20)
    const uncompressedSize = buf.readUInt32LE(cursor + 24)
    const nameLen = buf.readUInt16LE(cursor + 28)
    const extraLen = buf.readUInt16LE(cursor + 30)
    const commentLen = buf.readUInt16LE(cursor + 32)
    const localOffset = buf.readUInt32LE(cursor + 42)

    const inicioNombre = cursor + 46
    if (inicioNombre + nameLen > buf.length) {
      return fallo(`el nombre de la entrada ${i + 1} esta truncado`)
    }

    entradas.push({
      name: buf.toString('utf8', inicioNombre, inicioNombre + nameLen),
      method,
      flags,
      crc,
      compressedSize,
      uncompressedSize,
      localOffset,
    })

    cursor = inicioNombre + nameLen + extraLen + commentLen
  }

  return entradas
}

/** Descomprime una entrada y comprueba que lo que salio es lo que dice el indice. */
function extraerEntrada(buf: Buffer, entrada: EntradaCentral): Buffer | Fallo {
  if ((entrada.flags & FLAG_CIFRADO) !== 0) {
    return fallo(`"${entrada.name}" va cifrada: no se soportan contrasenas`)
  }
  if (entrada.compressedSize === ZIP64 || entrada.localOffset === ZIP64) {
    return fallo(`"${entrada.name}" usa ZIP64, que no se soporta`)
  }
  if (entrada.method !== ALMACENADO && entrada.method !== DEFLATE) {
    return fallo(
      `"${entrada.name}" usa el metodo de compresion ${entrada.method}: solo se soportan almacenado (0) y deflate (8)`,
    )
  }

  const cabecera = entrada.localOffset
  if (cabecera + 30 > buf.length) {
    return fallo(`la cabecera local de "${entrada.name}" queda fuera del archivo`)
  }
  if (buf.readUInt32LE(cabecera) !== LOCAL) {
    return fallo(`la cabecera local de "${entrada.name}" tiene una firma invalida`)
  }

  // De la cabecera local solo se usan las dos longitudes: los tamanos pueden
  // estar a cero si el ZIP se escribio en streaming, pero la geometria de la
  // cabecera siempre es la misma.
  const nameLen = buf.readUInt16LE(cabecera + 26)
  const extraLen = buf.readUInt16LE(cabecera + 28)
  const inicio = cabecera + 30 + nameLen + extraLen
  const fin = inicio + entrada.compressedSize

  if (fin > buf.length) {
    return fallo(`los datos de "${entrada.name}" estan truncados`)
  }

  const comprimido = buf.subarray(inicio, fin)

  let datos: Buffer
  if (entrada.method === ALMACENADO) {
    datos = Buffer.from(comprimido)
  } else {
    try {
      datos = inflateRawSync(comprimido)
    } catch (error) {
      return fallo(`no se pudo descomprimir "${entrada.name}": ${mensajeDe(error)}`)
    }
  }

  if (datos.length !== entrada.uncompressedSize) {
    return fallo(
      `"${entrada.name}" ocupa ${datos.length} bytes y el indice dice ${entrada.uncompressedSize}`,
    )
  }

  // El CRC es la unica comprobacion que detecta un archivo danado que aun asi
  // se descomprime. Sin esto, un byte malo saldria como una imagen con basura.
  const real = crc32(datos)
  if (real !== entrada.crc) {
    return fallo(
      `"${entrada.name}" esta danada: su CRC es ${real.toString(16)} y el indice dice ${entrada.crc.toString(16)}`,
    )
  }

  return datos
}

/**
 * Nombre con el que se guarda la entrada.
 *
 * Se descarta el directorio a proposito: un ZIP puede traer rutas
 * (`carpeta/pagina.jpg`) y respetarlas dejaria que un archivo escribiera fuera
 * del directorio de extraccion con un `../`. Solo se conserva el nombre base, y
 * los caracteres que no valen en un nombre de archivo se sustituyen.
 */
function nombreSeguro(name: string): string {
  return basename(name.replace(/\\/g, '/')).replace(/[<>:"|?*\u0000-\u001f]/g, '_')
}

/** Entradas que nunca son una pagina: carpetas, basura de macOS y metadatos. */
function esBasura(name: string): boolean {
  const normalizado = name.replace(/\\/g, '/')
  if (normalizado.endsWith('/')) return true
  if (normalizado.startsWith('__MACOSX/')) return true

  const base = basename(normalizado)
  return base === '' || base.startsWith('.')
}

function esImagen(name: string, extensiones: readonly string[]): boolean {
  const ext = extname(name).replace(/^\./, '').toLowerCase()
  return ext !== '' && extensiones.includes(ext)
}

/** Evita que dos entradas con el mismo nombre base se pisen al extraer. */
function nombreUnico(name: string, usados: Set<string>): string {
  if (!usados.has(name)) {
    usados.add(name)
    return name
  }

  const ext = extname(name)
  const raiz = ext === '' ? name : name.slice(0, -ext.length)

  let n = 2
  while (usados.has(`${raiz} (${n})${ext}`)) n++

  const unico = `${raiz} (${n})${ext}`
  usados.add(unico)
  return unico
}

/** Una imagen extraida del archivo. */
export interface ArchiveImage {
  /** Nombre que tenia dentro del archivo comprimido. */
  name: string
  /** Ruta absoluta del archivo extraido. */
  path: string
}

export interface ExpandRequest {
  archivePath: string
  /** Directorio donde extraer. Lo crea si no existe. */
  destDir: string
  /**
   * Extensiones que cuentan como imagen. Las pasa quien llama para que la lista
   * viva en un unico sitio (`packages/shared/src/types.ts`) en vez de tener una
   * copia aqui que se quede atras.
   */
  imageExtensions: readonly string[]
}

export type ExpandResult =
  | { status: 'expanded'; images: ArchiveImage[] }
  | { status: 'failed'; errorCode: ErrorCode; message: string }

/**
 * Extrae las imagenes de un ZIP o CBZ.
 *
 * Devuelve las imagenes en el **orden del directorio central**, que es el orden
 * en que se anadieron. Reordenar por nombre romperia un CBZ cuyas paginas no
 * lleven numero, que es justo el caso en el que el orden importa.
 *
 * No lanza: un fallo se devuelve como `failed` con su codigo, para que quien
 * llama decida como contarlo al usuario.
 */
export async function expandArchive(request: ExpandRequest): Promise<ExpandResult> {
  let buf: Buffer
  try {
    buf = await readFile(request.archivePath)
  } catch (error) {
    return fallo(`no se pudo leer el archivo: ${mensajeDe(error)}`)
  }

  const eocd = localizarEocd(buf)
  if (typeof eocd !== 'number') return eocd

  const entradas = leerDirectorioCentral(buf, eocd)
  if (!Array.isArray(entradas)) return entradas

  const paginas = entradas.filter(
    (entrada) => !esBasura(entrada.name) && esImagen(entrada.name, request.imageExtensions),
  )

  if (paginas.length === 0) {
    return fallo(
      `el archivo no contiene ninguna imagen: ${entradas.length} entrada(s), ninguna con extension de imagen`,
      'SU-E001',
    )
  }

  try {
    await mkdir(request.destDir, { recursive: true })
  } catch (error) {
    return fallo(`no se pudo crear el directorio de extraccion: ${mensajeDe(error)}`)
  }

  const images: ArchiveImage[] = []
  const usados = new Set<string>()

  for (const entrada of paginas) {
    const datos = extraerEntrada(buf, entrada)
    if (!Buffer.isBuffer(datos)) return datos

    const destino = nombreUnico(nombreSeguro(entrada.name), usados)
    const path = join(request.destDir, destino)

    try {
      await writeFile(path, datos)
    } catch (error) {
      return fallo(`no se pudo escribir "${destino}": ${mensajeDe(error)}`)
    }

    images.push({ name: entrada.name, path })
  }

  return { status: 'expanded', images }
}
