/**
 * Tests del descargador de modelos.
 *
 * Se ejecutan con `node --test` sobre el TypeScript directamente (Node 22 borra
 * los tipos sin compilar nada), y **no** necesitan Electron ni un sidecar: el
 * módulo es puro. Cada test levanta un servidor HTTP de verdad en un puerto
 * libre, porque lo que hay que comprobar es el comportamiento frente a un
 * servidor real (rangos, respuestas a medias, conexiones mudas), no frente a un
 * doble de pruebas que se comporta como yo creo que se comporta un servidor.
 *
 * Comando: `npm run test:main`
 */

import { test, type TestContext } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync } from 'node:fs'
import { mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { Socket } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { downloadModel, sha256File, PART_SUFFIX, type DownloadProgress } from './downloader.ts'

// --- Utilidades --------------------------------------------------------------

function sha256(data: Uint8Array): string {
  return createHash('sha256').update(data).digest('hex')
}

type Handler = (req: IncomingMessage, res: ServerResponse) => void

interface TestServer {
  url: string
  close: () => Promise<void>
}

/**
 * Servidor de un solo uso en un puerto libre.
 *
 * Los sockets se registran y se destruyen a mano al cerrar. `closeAllConnections`
 * no basta cuando una respuesta se quedó a medias a propósito: el socket sigue
 * vivo, el bucle de eventos con él, y `node --test` no termina nunca.
 */
async function serve(handler: Handler): Promise<TestServer> {
  const sockets = new Set<Socket>()
  const server = createServer(handler)

  server.on('connection', (socket) => {
    sockets.add(socket)
    socket.on('close', () => sockets.delete(socket))
  })

  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  const port = typeof address === 'object' && address !== null ? address.port : 0

  return {
    url: `http://127.0.0.1:${port}/modelo.onnx`,
    close: () =>
      new Promise<void>((resolve) => {
        for (const socket of sockets) socket.destroy()
        sockets.clear()
        server.close(() => resolve())
      }),
  }
}

/** Sirve un buffer, atendiendo `Range` como un servidor de archivos de verdad. */
function servePayload(payload: Buffer, onRequest?: (range?: string) => void): Handler {
  return (req, res) => {
    const range = req.headers.range
    onRequest?.(range)

    const from = range ? Number(/bytes=(\d+)-/.exec(range)?.[1] ?? 0) : 0
    const slice = payload.subarray(from)
    const headers: Record<string, string> = { 'content-length': String(slice.length) }
    if (from > 0) headers['content-range'] = `bytes ${from}-${payload.length - 1}/${payload.length}`

    res.writeHead(from > 0 ? 206 : 200, headers)
    res.end(slice)
  }
}

/**
 * Directorio temporal y servidores del test, con limpieza garantizada.
 *
 * La limpieza se registra **antes** de nada. Si una aserción falla, el servidor
 * se cierra igualmente: sin esto, un test rojo deja un puerto escuchando y
 * `node --test` no termina nunca. Un cuelgue es peor que un fallo, porque
 * esconde el fallo.
 */
async function harness(t: TestContext) {
  const dir = await mkdtemp(join(tmpdir(), 'su-downloader-'))
  const servers: TestServer[] = []

  t.after(async () => {
    for (const server of servers) await server.close()
    await rm(dir, { recursive: true, force: true }).catch(() => undefined)
  })

  return {
    /** Ruta dentro del directorio temporal del test. */
    path: (name: string) => join(dir, name),
    serve: async (handler: Handler): Promise<TestServer> => {
      const server = await serve(handler)
      servers.push(server)
      return server
    },
  }
}

// --- El camino feliz --------------------------------------------------------

test('descarga, comprueba el hash y deja el archivo en su sitio', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(256 * 1024, 7)
  const hash = sha256(payload)
  const server = await h.serve(servePayload(payload))
  const dest = h.path('modelo.onnx')
  const seen: DownloadProgress[] = []

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: hash,
    sizeBytes: payload.length,
    destPath: dest,
    progressMs: 0,
    onProgress: (update) => seen.push(update),
  })

  assert.equal(result.status, 'completed')
  assert.deepEqual(await readFile(dest), payload)
  // No queda basura a medio escribir con el nombre definitivo ya puesto.
  assert.equal(existsSync(dest + PART_SUFFIX), false)
  // Y el archivo final es, byte a byte, el que se pidió.
  assert.equal(await sha256File(dest), hash)

  assert.ok(seen.length > 0, 'deberia avisar del progreso')
  const last = seen[seen.length - 1]
  assert.ok(last, 'hay al menos un aviso')
  assert.equal(last.receivedBytes, payload.length)
  assert.equal(last.ratio, 1)
  assert.equal(last.url, server.url)
})

test('crea el directorio de destino si no existe', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(4096, 1)
  const server = await h.serve(servePayload(payload))
  // Dos niveles que no existen: el directorio de modelos lo crea la aplicación
  // en el primer arranque, pero la descarga no debería depender de eso.
  const dest = join(h.path('raiz'), 'no', 'existe', 'modelo.onnx')

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: dest,
  })

  assert.equal(result.status, 'completed')
  assert.equal((await stat(dest)).size, payload.length)
})

// --- Verificación del hash --------------------------------------------------

test('un hash que no coincide no deja el modelo instalado', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(8192, 5)
  const server = await h.serve(servePayload(payload))
  const dest = h.path('modelo.onnx')

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    // El hash de otro archivo: es el caso de un espejo comprometido o de una
    // descarga corrompida. Ni se instala ni se deja el .part.
    sha256: sha256(Buffer.alloc(8192, 6)),
    sizeBytes: payload.length,
    destPath: dest,
  })

  assert.equal(result.status, 'failed')
  assert.equal(result.status === 'failed' ? result.errorCode : null, 'SU-E111')
  assert.equal(existsSync(dest), false, 'un archivo que no cuadra no es un modelo')
  assert.equal(existsSync(dest + PART_SUFFIX), false, 'y no se guarda para reanudar')
})

test('sha256File calcula el hash que dice el estándar', async (t) => {
  const h = await harness(t)
  const empty = h.path('vacio.bin')
  await writeFile(empty, '')

  // Vector conocido: el sha256 de la cadena vacía. Si esto cambia, el problema
  // no es el test.
  assert.equal(
    await sha256File(empty),
    'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
  )
})

test('acepta el hash del manifiesto escrito en mayusculas', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(4096, 2)
  const server = await h.serve(servePayload(payload))

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload).toUpperCase(),
    sizeBytes: payload.length,
    destPath: h.path('modelo.onnx'),
  })

  // La comparación no distingue mayúsculas: un hash en mayúsculas es el mismo
  // hash, y rechazarlo sería un falso positivo que el usuario no entendería.
  assert.equal(result.status, 'completed')
})

// --- Reanudación ------------------------------------------------------------

test('reanuda desde el .part en lugar de empezar de cero', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(200 * 1024, 3)
  const half = 100 * 1024
  let rangeHeader: string | undefined

  const server = await h.serve(
    servePayload(payload, (range) => {
      rangeHeader = range
    }),
  )
  const dest = h.path('modelo.onnx')
  // Una descarga anterior que se quedó a medias.
  await writeFile(dest + PART_SUFFIX, payload.subarray(0, half))

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: dest,
  })

  assert.equal(result.status, 'completed')
  assert.equal(rangeHeader, `bytes=${half}-`, 'deberia pedir solo lo que falta')
  // El archivo final está entero y bien: lo reanudado y lo nuevo encajan.
  assert.deepEqual(await readFile(dest), payload)
  assert.equal(existsSync(dest + PART_SUFFIX), false)
})

test('si el servidor ignora el rango, empieza de cero en lugar de corromper el archivo', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(64 * 1024, 9)
  // Un servidor que responde 200 con el archivo entero aunque se le pida un rango.
  const server = await h.serve((_req, res) => {
    res.writeHead(200, { 'content-length': String(payload.length) })
    res.end(payload)
  })
  const dest = h.path('modelo.onnx')
  await writeFile(dest + PART_SUFFIX, payload.subarray(0, 32 * 1024))

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: dest,
  })

  // Añadir la respuesta entera al final del .part daría un archivo con los
  // primeros 32 KB duplicados y el hash fallaría: hay que truncar.
  assert.equal(result.status, 'completed')
  assert.deepEqual(await readFile(dest), payload)
})

test('un .part mas grande que el archivo remoto se descarta y se reintenta contra el mismo espejo', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(4096, 4)
  let requests = 0

  const server = await h.serve((req, res) => {
    requests += 1
    // El servidor rechaza el rango la primera vez, como haría uno cuyo archivo
    // ha encogido, y luego lo sirve entero.
    if (requests === 1) {
      res.writeHead(416, { 'content-range': `bytes */${payload.length}` })
      res.end()
      return
    }
    servePayload(payload)(req, res)
  })

  const dest = h.path('modelo.onnx')
  // Un .part absurdo: más grande que el archivo que hay al otro lado.
  await writeFile(dest + PART_SUFFIX, Buffer.alloc(9000, 0))

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: dest,
  })

  // Un parcial obsoleto no debería obligar al usuario a buscar otro espejo ni a
  // pulsar descargar una segunda vez: se recupera solo.
  assert.equal(result.status, 'completed', 'deberia recuperarse sin cambiar de espejo')
  assert.equal(requests, 2, 'un intento con rango y otro entero')
  assert.deepEqual(await readFile(dest), payload)
  assert.equal(existsSync(dest + PART_SUFFIX), false)
})

// --- Espejos ----------------------------------------------------------------

test('si un espejo falla, usa el siguiente', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(32 * 1024, 8)
  const broken = await h.serve((_req, res) => {
    res.writeHead(503)
    res.end('no disponible')
  })
  const good = await h.serve(servePayload(payload))
  const dest = h.path('modelo.onnx')

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [broken.url, good.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: dest,
  })

  assert.equal(result.status, 'completed')
  assert.deepEqual(await readFile(dest), payload)
})

test('si todos los espejos fallan, el mensaje dice cual y por que', async (t) => {
  const h = await harness(t)
  const broken = await h.serve((_req, res) => {
    res.writeHead(404)
    res.end()
  })

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [broken.url],
    sha256: sha256(Buffer.from('x')),
    sizeBytes: 1,
    destPath: h.path('modelo.onnx'),
  })

  assert.equal(result.status, 'failed')
  assert.equal(result.status === 'failed' ? result.errorCode : null, 'SU-E112')
  const message = result.status === 'failed' ? result.message : ''
  // Sin la URL no se puede distinguir "sin conexión" de "el espejo se movió".
  assert.match(message, /HTTP 404/)
  assert.ok(message.includes(broken.url), `deberia nombrar el espejo: ${message}`)
})

test('un modelo sin URL no se intenta descargar', async (t) => {
  const h = await harness(t)

  const result = await downloadModel({
    modelId: 'sin-url',
    urls: [],
    sha256: sha256(Buffer.from('x')),
    sizeBytes: 1,
    destPath: h.path('modelo.onnx'),
  })

  assert.equal(result.status, 'failed')
  assert.equal(result.status === 'failed' ? result.errorCode : null, 'SU-E112')
  assert.match(result.status === 'failed' ? result.message : '', /ninguna URL/)
})

// --- Cancelación y conexiones mudas -----------------------------------------

test('cancelar conserva el .part para poder reanudar', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(512 * 1024, 1)
  const controller = new AbortController()

  const server = await h.serve((_req, res) => {
    res.writeHead(200, { 'content-length': String(payload.length) })
    res.write(payload.subarray(0, 32 * 1024))
    // A partir de aquí no manda nada y no cierra: sin el abort, esto no acaba.
  })

  const dest = h.path('modelo.onnx')

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: dest,
    signal: controller.signal,
    progressMs: 0,
    onProgress: () => controller.abort(),
  })

  assert.equal(result.status, 'cancelled')
  // El modelo no se instala a medias...
  assert.equal(existsSync(dest), false)
  // ...pero lo descargado se conserva: es justo lo que permite reanudar.
  assert.ok(existsSync(dest + PART_SUFFIX), 'el .part es la mitad del trabajo ya hecho')
  assert.equal((await stat(dest + PART_SUFFIX)).size, 32 * 1024)
})

test('cancelar antes de empezar no toca el disco', async (t) => {
  const h = await harness(t)
  const controller = new AbortController()
  controller.abort()
  const dest = h.path('modelo.onnx')

  const result = await downloadModel({
    modelId: 'prueba',
    urls: ['http://127.0.0.1:1/no-existe.onnx'],
    sha256: sha256(Buffer.from('x')),
    sizeBytes: 1,
    destPath: dest,
    signal: controller.signal,
  })

  assert.equal(result.status, 'cancelled')
  assert.equal(existsSync(dest), false)
})

test('una conexion que se queda muda se da por perdida en vez de colgarse', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(1024, 1)
  const server = await h.serve((_req, res) => {
    res.writeHead(200, { 'content-length': String(payload.length) })
    // `flushHeaders` es lo que hace que esto sea "manda cabeceras y luego calla"
    // en lugar de "no contesta nada". Sin él, Node retiene las cabeceras hasta el
    // primer byte de cuerpo, y el caso que se prueba es el otro.
    res.flushHeaders()
  })

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: h.path('modelo.onnx'),
    // Sin esto, el test duraría el minuto del valor por defecto.
    stallMs: 150,
  })

  assert.equal(result.status, 'failed')
  assert.equal(result.status === 'failed' ? result.errorCode : null, 'SU-E112')
  assert.match(result.status === 'failed' ? result.message : '', /sin datos/)
})

test('un servidor que acepta la conexion y no contesta nunca no cuelga la descarga', async (t) => {
  const h = await harness(t)
  const payload = Buffer.alloc(1024, 1)
  // Ni cabeceras: la petición se queda esperando indefinidamente. Es lo que hace
  // un servidor saturado, y el caso que se colgaba: el temporizador de
  // inactividad solo miraba el cuerpo, así que la espera de la respuesta no
  // tenía límite y la descarga se quedaba muda para siempre.
  const server = await h.serve(() => {
    /* a propósito: no responde nada */
  })

  const result = await downloadModel({
    modelId: 'prueba',
    urls: [server.url],
    sha256: sha256(payload),
    sizeBytes: payload.length,
    destPath: h.path('modelo.onnx'),
    stallMs: 150,
  })

  assert.equal(result.status, 'failed')
  assert.equal(result.status === 'failed' ? result.errorCode : null, 'SU-E112')
  assert.match(result.status === 'failed' ? result.message : '', /sin datos/)
})
