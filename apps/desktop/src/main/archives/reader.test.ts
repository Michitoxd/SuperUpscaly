/**
 * Pruebas del lector de ZIP y CBZ.
 *
 * Los archivos de prueba se **construyen a mano** en lugar de usar uno de
 * ejemplo. Es a proposito: si se usara una herramienta para crearlos y el mismo
 * tipo de herramienta para leerlos, un malentendido del formato compartido por
 * las dos pasaria desapercibido. Escribiendo los bytes aqui, la prueba comprueba
 * el lector contra el formato, no contra otro programa.
 *
 * Se levantan ficheros de verdad en el directorio temporal del sistema: lo que
 * se prueba incluye que el CRC cuadre y que los archivos acaben escritos.
 */

import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { crc32, deflateRawSync } from 'node:zlib'

import { expandArchive } from './reader.ts'

/** Las mismas que `IMAGE_EXTENSIONS` de `packages/shared/src/types.ts`. */
const EXTENSIONES = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff', 'avif']

const DEFLATE = 8

/**
 * Contenido de relleno para las entradas.
 *
 * No es un PNG valido y no hace falta que lo sea: el lector no interpreta las
 * imagenes, solo comprueba la extension para decidir que extrae. Lo que se
 * verifica es que los bytes que salen son **exactamente** los que entraron.
 */
const CONTENIDO = Buffer.from('bytes de relleno que representan una imagen', 'utf8')

interface EntradaDePrueba {
  name: string
  data: Buffer
  /** 0 = almacenado, 8 = deflate. Cualquier otro valor se escribe tal cual. */
  method?: number
  flags?: number
  /** Escribe un CRC que no corresponde, para probar la deteccion. */
  crcRoto?: boolean
}

/** Construye un ZIP minimo pero valido, con directorio central y EOCD. */
function construirZip(entradas: EntradaDePrueba[]): Buffer {
  const locales: Buffer[] = []
  const centrales: Buffer[] = []
  let offset = 0

  for (const entrada of entradas) {
    const name = Buffer.from(entrada.name, 'utf8')
    const method = entrada.method ?? 8
    const flags = entrada.flags ?? 0
    const comprimido = method === DEFLATE ? deflateRawSync(entrada.data) : entrada.data

    const crc = entrada.crcRoto ? (crc32(entrada.data) ^ 0xffffffff) >>> 0 : crc32(entrada.data) >>> 0

    const local = Buffer.alloc(30 + name.length)
    local.writeUInt32LE(0x04034b50, 0)
    local.writeUInt16LE(20, 4)
    local.writeUInt16LE(flags, 6)
    local.writeUInt16LE(method, 8)
    local.writeUInt32LE(crc, 14)
    local.writeUInt32LE(comprimido.length, 18)
    local.writeUInt32LE(entrada.data.length, 22)
    local.writeUInt16LE(name.length, 26)
    name.copy(local, 30)

    locales.push(local, comprimido)

    const central = Buffer.alloc(46 + name.length)
    central.writeUInt32LE(0x02014b50, 0)
    central.writeUInt16LE(20, 4)
    central.writeUInt16LE(20, 6)
    central.writeUInt16LE(flags, 8)
    central.writeUInt16LE(method, 10)
    central.writeUInt32LE(crc, 16)
    central.writeUInt32LE(comprimido.length, 20)
    central.writeUInt32LE(entrada.data.length, 24)
    central.writeUInt16LE(name.length, 28)
    central.writeUInt32LE(offset, 42)
    name.copy(central, 46)

    centrales.push(central)
    offset += local.length + comprimido.length
  }

  const directorio = Buffer.concat(centrales)

  const eocd = Buffer.alloc(22)
  eocd.writeUInt32LE(0x06054b50, 0)
  eocd.writeUInt16LE(entradas.length, 8)
  eocd.writeUInt16LE(entradas.length, 10)
  eocd.writeUInt32LE(directorio.length, 12)
  eocd.writeUInt32LE(offset, 16)

  return Buffer.concat([...locales, directorio, eocd])
}

/** Escribe `valor` en un campo del EOCD, que siempre son los ultimos 22 bytes. */
function parchearEocd(zip: Buffer, desplazamiento: number, valor: number, bytes: 2 | 4): Buffer {
  const copia = Buffer.from(zip)
  const base = copia.length - 22
  if (bytes === 2) copia.writeUInt16LE(valor, base + desplazamiento)
  else copia.writeUInt32LE(valor, base + desplazamiento)
  return copia
}

/**
 * Prepara un directorio temporal y devuelve el escritor de archivos de prueba.
 *
 * La limpieza se registra **antes** de cualquier asercion: si una prueba falla,
 * el directorio se borra igual.
 */
async function harness(t: { after: (fn: () => Promise<void>) => void }) {
  const raiz = await mkdtemp(join(tmpdir(), 'su-zip-'))

  t.after(async () => {
    await rm(raiz, { recursive: true, force: true }).catch(() => undefined)
  })

  async function escribir(nombre: string, contenido: Buffer): Promise<string> {
    const path = join(raiz, nombre)
    await writeFile(path, contenido)
    return path
  }

  return {
    raiz,
    destino: join(raiz, 'extraido'),
    escribir,
    async escribirZip(nombre: string, entradas: EntradaDePrueba[]): Promise<string> {
      return escribir(nombre, construirZip(entradas))
    },
  }
}

test('extrae las imagenes de un archivo comprimido con deflate', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('paginas.cbz', [
    { name: '001.png', data: CONTENIDO },
    { name: '002.png', data: CONTENIDO },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return

  assert.equal(resultado.images.length, 2)
  assert.deepEqual(
    resultado.images.map((imagen) => imagen.name),
    ['001.png', '002.png'],
  )

  // Y lo que se escribio es la imagen, no algo corrupto.
  for (const imagen of resultado.images) {
    assert.deepEqual(await readFile(imagen.path), CONTENIDO)
  }
})

test('extrae entradas almacenadas sin comprimir', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('almacenado.zip', [
    { name: 'foto.jpg', data: CONTENIDO, method: 0 },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return
  assert.deepEqual(await readFile(resultado.images[0]!.path), CONTENIDO)
})

test('crea el directorio de extraccion si no existe', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('paginas.zip', [{ name: 'a.png', data: CONTENIDO }])

  assert.equal(existsSync(h.destino), false)

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  assert.equal(existsSync(h.destino), true)
})

test('conserva el orden del directorio central, no el alfabetico', async (t) => {
  const h = await harness(t)
  // Nombres sin numero: si se reordenara por nombre, el orden de lectura se
  // rompe. El orden del indice es el orden de las paginas.
  const zip = await h.escribirZip('comic.cbz', [
    { name: 'zeta.png', data: CONTENIDO },
    { name: 'alfa.png', data: CONTENIDO },
    { name: 'media.png', data: CONTENIDO },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return
  assert.deepEqual(
    resultado.images.map((imagen) => imagen.name),
    ['zeta.png', 'alfa.png', 'media.png'],
  )
})

test('ignora lo que no es una imagen', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('mixto.zip', [
    { name: 'leeme.txt', data: Buffer.from('hola') },
    { name: '001.png', data: CONTENIDO },
    { name: 'comic.xml', data: Buffer.from('<ComicInfo/>') },
    { name: '002.png', data: CONTENIDO },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return
  assert.deepEqual(
    resultado.images.map((imagen) => imagen.name),
    ['001.png', '002.png'],
  )
})

test('descarta la basura de macOS, las carpetas y los archivos ocultos', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('mac.zip', [
    { name: '__MACOSX/._001.png', data: CONTENIDO },
    { name: 'carpeta/', data: Buffer.alloc(0) },
    { name: '.oculto.png', data: CONTENIDO },
    { name: '001.png', data: CONTENIDO },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return
  assert.deepEqual(
    resultado.images.map((imagen) => imagen.name),
    ['001.png'],
  )
})

test('aplana las rutas internas en lugar de respetarlas', async (t) => {
  const h = await harness(t)
  // Un ZIP puede traer rutas con `..`. Respetarlas dejaria que el archivo
  // escribiera fuera del directorio de extraccion.
  const zip = await h.escribirZip('rutas.zip', [
    { name: 'paginas/001.png', data: CONTENIDO },
    { name: '../../fuera.png', data: CONTENIDO },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return

  for (const imagen of resultado.images) {
    assert.equal(
      imagen.path.startsWith(h.destino),
      true,
      `"${imagen.path}" deberia estar dentro de ${h.destino}`,
    )
  }

  assert.equal(existsSync(join(h.raiz, 'fuera.png')), false, 'no deberia escribir fuera')
  assert.equal(existsSync(join(h.destino, '001.png')), true)
  assert.equal(existsSync(join(h.destino, 'fuera.png')), true)
})

test('renombra las entradas que comparten nombre base', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('duplicados.zip', [
    { name: 'a/pagina.png', data: CONTENIDO },
    { name: 'b/pagina.png', data: CONTENIDO },
    { name: 'c/pagina.png', data: CONTENIDO },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'expanded')
  if (resultado.status !== 'expanded') return

  const nombres = resultado.images.map((imagen) => imagen.path.split(/[\\/]/).pop())
  assert.deepEqual(nombres, ['pagina.png', 'pagina (2).png', 'pagina (3).png'])

  // Las tres tienen que seguir existiendo: ninguna se ha pisado.
  for (const imagen of resultado.images) {
    assert.equal(existsSync(imagen.path), true)
    assert.deepEqual(await readFile(imagen.path), CONTENIDO)
  }
})

test('un archivo que no es un ZIP falla con el codigo de archivo corrupto', async (t) => {
  const h = await harness(t)
  const path = await h.escribir('no-es-un-zip.zip', Buffer.from('esto no es un zip en absoluto'))

  const resultado = await expandArchive({
    archivePath: path,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
  assert.match(resultado.message, /ZIP/)
})

test('un ZIP truncado falla en vez de extraer medio archivo', async (t) => {
  const h = await harness(t)
  const completo = construirZip([{ name: '001.png', data: CONTENIDO }])
  const truncado = completo.subarray(0, Math.floor(completo.length / 2))
  const path = await h.escribir('truncado.zip', truncado)

  const resultado = await expandArchive({
    archivePath: path,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
})

test('un CRC que no cuadra falla aunque los datos se descompriman', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('danado.zip', [
    { name: '001.png', data: CONTENIDO, crcRoto: true },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
  assert.match(resultado.message, /CRC/)
  assert.match(resultado.message, /001\.png/)
})

test('un metodo de compresion no soportado se dice, no se intenta', async (t) => {
  const h = await harness(t)
  // 12 es bzip2: descomprimirlo sin la biblioteca daria basura.
  const zip = await h.escribirZip('bzip2.zip', [{ name: '001.png', data: CONTENIDO, method: 12 }])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
  assert.match(resultado.message, /12/)
})

test('una entrada cifrada se rechaza en vez de producir basura', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('cifrado.zip', [
    { name: '001.png', data: CONTENIDO, flags: 0x0001 },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
  assert.match(resultado.message, /cifrada/)
})

test('un archivo ZIP64 se rechaza con su motivo', async (t) => {
  const h = await harness(t)
  // El desplazamiento del directorio central a 0xFFFFFFFF es la marca de ZIP64.
  const zip64 = parchearEocd(
    construirZip([{ name: '001.png', data: CONTENIDO }]),
    16,
    0xffffffff,
    4,
  )
  const path = await h.escribir('zip64.zip', zip64)

  const resultado = await expandArchive({
    archivePath: path,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
  assert.match(resultado.message, /ZIP64/)
})

test('un archivo sin imagenes lo dice en vez de crear un trabajo vacio', async (t) => {
  const h = await harness(t)
  const zip = await h.escribirZip('solo-texto.zip', [
    { name: 'leeme.txt', data: Buffer.from('hola') },
    { name: 'datos.xml', data: Buffer.from('<x/>') },
  ])

  const resultado = await expandArchive({
    archivePath: zip,
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E001')
  assert.match(resultado.message, /2 entrada/)
})

test('un archivo que no existe falla sin lanzar', async (t) => {
  const h = await harness(t)

  const resultado = await expandArchive({
    archivePath: join(h.raiz, 'no-existe.zip'),
    destDir: h.destino,
    imageExtensions: EXTENSIONES,
  })

  assert.equal(resultado.status, 'failed')
  if (resultado.status !== 'failed') return
  assert.equal(resultado.errorCode, 'SU-E102')
})
