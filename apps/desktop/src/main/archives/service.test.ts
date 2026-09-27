/**
 * Pruebas de la expansion de comprimidos dentro de una lista de trabajo.
 *
 * Lo que se comprueba aqui es el **contrato de la lista**, no el lector: que un
 * comprimido se sustituya en su sitio por sus paginas y que uno que no se puede
 * abrir se quede donde estaba. Ese segundo caso es el que importa: si el archivo
 * desapareciera de la lista, la cola cambiaria de tamano bajo los dedos del
 * usuario y el fallo no tendria donde contarse.
 *
 * Los ZIP de prueba se construyen a mano, con entradas almacenadas (metodo 0),
 * por el mismo motivo que en `reader.test.ts`: asi la prueba comprueba el
 * contrato contra el formato y no contra otra herramienta.
 */

import assert from 'node:assert/strict'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { crc32 } from 'node:zlib'

import { ArchiveService } from './service.ts'

const EXTENSIONES = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff', 'avif']

interface EntradaDePrueba {
  name: string
  data: Buffer
}

/** ZIP minimo valido con entradas almacenadas: cabeceras, directorio y EOCD. */
function construirZip(entradas: EntradaDePrueba[]): Buffer {
  const partes: Buffer[] = []
  const centrales: Buffer[] = []
  let offset = 0

  for (const entrada of entradas) {
    const name = Buffer.from(entrada.name, 'utf8')
    const crc = crc32(entrada.data) >>> 0

    const local = Buffer.alloc(30 + name.length)
    local.writeUInt32LE(0x04034b50, 0)
    local.writeUInt16LE(20, 4)
    local.writeUInt16LE(0, 8) // metodo 0: almacenado
    local.writeUInt32LE(crc, 14)
    local.writeUInt32LE(entrada.data.length, 18)
    local.writeUInt32LE(entrada.data.length, 22)
    local.writeUInt16LE(name.length, 26)
    name.copy(local, 30)

    partes.push(local, entrada.data)

    const central = Buffer.alloc(46 + name.length)
    central.writeUInt32LE(0x02014b50, 0)
    central.writeUInt16LE(20, 4)
    central.writeUInt16LE(20, 6)
    central.writeUInt16LE(0, 10) // metodo 0
    central.writeUInt32LE(crc, 16)
    central.writeUInt32LE(entrada.data.length, 20)
    central.writeUInt32LE(entrada.data.length, 24)
    central.writeUInt16LE(name.length, 28)
    central.writeUInt32LE(offset, 42)
    name.copy(central, 46)

    centrales.push(central)
    offset += local.length + entrada.data.length
  }

  const tamanoDirectorio = centrales.reduce((suma, central) => suma + central.length, 0)

  const eocd = Buffer.alloc(22)
  eocd.writeUInt32LE(0x06054b50, 0)
  eocd.writeUInt16LE(entradas.length, 8)
  eocd.writeUInt16LE(entradas.length, 10)
  eocd.writeUInt32LE(tamanoDirectorio, 12)
  eocd.writeUInt32LE(offset, 16)

  return Buffer.concat([...partes, ...centrales, eocd])
}

const IMAGEN = Buffer.from('bytes que representan una pagina', 'utf8')

async function enCarpetaDePrueba(
  proof: (dir: string) => Promise<void>,
): Promise<void> {
  const dir = await mkdtemp(join(tmpdir(), 'su-archivos-'))
  try {
    await proof(dir)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

test('las paginas de un comprimido entran donde estaba el comprimido', async () => {
  await enCarpetaDePrueba(async (dir) => {
    const antes = join(dir, 'antes.png')
    const despues = join(dir, 'despues.png')
    const cbz = join(dir, 'libro.cbz')

    await writeFile(antes, IMAGEN)
    await writeFile(despues, IMAGEN)
    await writeFile(
      cbz,
      construirZip([
        { name: 'pagina-01.png', data: IMAGEN },
        { name: 'pagina-02.png', data: IMAGEN },
      ]),
    )

    const servicio = new ArchiveService(join(dir, 'extraido'))
    const resultado = await servicio.expandAll([antes, cbz, despues])

    assert.equal(resultado.failed.length, 0)
    assert.equal(resultado.items.length, 4)
    assert.equal(resultado.items[0], antes)
    assert.equal(resultado.items[3], despues)
    // En su sitio y en el orden del archivo, no alfabetico ni al final.
    assert.match(resultado.items[1] ?? '', /pagina-01\.png$/)
    assert.match(resultado.items[2] ?? '', /pagina-02\.png$/)
  })
})

test('un comprimido que no se puede abrir se queda en la lista y dice por que', async () => {
  await enCarpetaDePrueba(async (dir) => {
    const roto = join(dir, 'roto.zip')
    await writeFile(roto, Buffer.from('esto no es un zip', 'utf8'))

    const servicio = new ArchiveService(join(dir, 'extraido'))
    const resultado = await servicio.expandAll([roto])

    assert.equal(resultado.items.length, 1)
    assert.equal(resultado.items[0], roto)
    assert.equal(resultado.failed.length, 1)
    assert.equal(resultado.failed[0]?.item, roto)
    assert.ok(resultado.failed[0]?.message && resultado.failed[0].message.length > 0)
  })
})

test('expandir dos veces el mismo comprimido da las mismas rutas', async () => {
  await enCarpetaDePrueba(async (dir) => {
    const cbz = join(dir, 'libro.cbz')
    await writeFile(cbz, construirZip([{ name: 'pagina.png', data: IMAGEN }]))

    const servicio = new ArchiveService(join(dir, 'extraido'))
    const primera = await servicio.expandAll([cbz])
    const segunda = await servicio.expandAll([cbz])

    assert.deepEqual(segunda.items, primera.items)
  })
})

test('una lista de solo imagenes no se toca', async () => {
  await enCarpetaDePrueba(async (dir) => {
    const a = join(dir, 'a.png')
    const b = join(dir, 'b.jpg')
    await writeFile(a, IMAGEN)
    await writeFile(b, IMAGEN)

    const servicio = new ArchiveService(join(dir, 'extraido'))
    const resultado = await servicio.expandAll([a, b])

    assert.deepEqual(resultado.items, [a, b])
    assert.equal(resultado.failed.length, 0)
    assert.equal(servicio.isArchive(a), false)
    assert.equal(servicio.isArchive(join(dir, 'x.cbz')), true)
    // La lista de extensiones la decide quien llama, no este modulo.
    assert.equal(EXTENSIONES.includes('png'), true)
  })
})
