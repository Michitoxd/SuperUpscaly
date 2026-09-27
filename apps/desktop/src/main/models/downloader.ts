/**
 * Descarga de modelos.
 *
 * Vive en el proceso principal y no en el sidecar a propósito:
 *
 *  - El sidecar es Rust y un cliente HTTP con TLS arrastraría `ring`, que es
 *    código C. Eso obliga a un compilador de C en las tres plataformas para algo
 *    que no lo necesita.
 *  - El proxy del sistema, el directorio de datos del usuario y el aviso de
 *    progreso a la interfaz son cosas de la capa de aplicación, no del motor.
 *  - Node trae `fetch`, `crypto` y streaming sin añadir una sola dependencia.
 *
 * El módulo es **puro**: solo importa `node:*` y tipos. Nada de Electron ni de
 * `@superupscaly/shared` en tiempo de ejecución (el paquete compartido apunta a
 * fuentes `.ts` con imports sin extensión, que Node no puede resolver). Gracias a
 * eso los tests se ejecutan de verdad con `node --test`, sin empaquetar nada.
 */

import { createHash } from 'node:crypto'
import { createWriteStream } from 'node:fs'
import { mkdir, open, rename, rm, stat } from 'node:fs/promises'
import { dirname } from 'node:path'
import type { ErrorCode } from '@superupscaly/shared'

/** Sufijo del archivo a medio escribir. Nunca se deja un `.part` como resultado. */
export const PART_SUFFIX = '.part'

/** Sin datos durante este tiempo, la descarga se da por muerta. */
const DEFAULT_STALL_MS = 60_000

/**
 * Cada cuánto se avisa del progreso.
 *
 * Un archivo de 340 MB llega en miles de trozos; reenviarlos todos cruzaría el
 * IPC miles de veces por segundo para pintar lo mismo. Los tests lo ponen a 0
 * para ver cada trozo.
 */
const DEFAULT_PROGRESS_MS = 150

export interface DownloadRequest {
  /** `id` del modelo, solo para los mensajes. */
  modelId: string
  /** Espejos en orden de preferencia. Se prueban hasta que uno funcione. */
  urls: readonly string[]
  /** `sha256` esperado, en hexadecimal. Obligatorio: sin hash no se descarga. */
  sha256: string
  /** Tamaño esperado en bytes. `0` = desconocido. */
  sizeBytes: number
  /** Ruta final. El archivo a medio escribir va al lado, con `.part`. */
  destPath: string
  /** Cancelación pedida por el usuario. */
  signal?: AbortSignal
  onProgress?: (progress: DownloadProgress) => void
  /** Milisegundos sin recibir datos antes de darse por vencido. */
  stallMs?: number
  /** Milisegundos entre avisos de progreso. `0` = en cada trozo. */
  progressMs?: number
}

export interface DownloadProgress {
  /** Bytes ya escritos, contando lo que se reanudó de una descarga anterior. */
  receivedBytes: number
  /** Total esperado. `0` si no se conoce. */
  totalBytes: number
  /** Espejo que se está usando. */
  url: string
  /** `receivedBytes / totalBytes`, o `null` si no se conoce el total. */
  ratio: number | null
  /** Velocidad media desde que empezó, en bytes por segundo. */
  bytesPerSecond: number | null
}

export type DownloadResult =
  | { status: 'completed'; bytes: number }
  | { status: 'cancelled' }
  | { status: 'failed'; errorCode: ErrorCode; message: string }

/**
 * Descarga un modelo, comprueba su hash y lo deja en su sitio.
 *
 * No lanza: un fallo de red no es una excepción del programa, es un resultado
 * que la interfaz tiene que contar. Devuelve siempre un `DownloadResult`.
 */
export async function downloadModel(request: DownloadRequest): Promise<DownloadResult> {
  if (request.urls.length === 0) {
    return fail('SU-E112', 'el manifiesto no declara ninguna URL para este modelo')
  }

  const partPath = `${request.destPath}${PART_SUFFIX}`

  try {
    await mkdir(dirname(request.destPath), { recursive: true })
  } catch (error) {
    return fail('SU-E150', `no se pudo preparar el directorio de modelos: ${describe(error)}`)
  }

  let lastMessage = 'no se intentó ninguna descarga'

  for (const url of request.urls) {
    if (request.signal?.aborted) return { status: 'cancelled' }

    const attempt = await attemptMirror(request, url, partPath)

    if (attempt.status === 'cancelled') return { status: 'cancelled' }
    if (attempt.status === 'failed') {
      // Un espejo caído no condena la descarga: queda el siguiente. El error se
      // guarda por si no queda ninguno más.
      lastMessage = `${url}: ${attempt.message}`
      continue
    }

    // El archivo está entero. Falta lo único que demuestra que es el modelo
    // correcto y no un archivo cualquiera con el nombre puesto.
    const actual = await sha256File(partPath).catch((error: unknown) => {
      lastMessage = `${url}: no se pudo leer lo descargado: ${describe(error)}`
      return null
    })
    // No se pudo leer el archivo: es un problema de este intento, no del modelo,
    // así que se deja probar el siguiente espejo.
    if (actual === null) continue

    if (actual !== request.sha256.toLowerCase()) {
      // Se borra: un archivo con el hash equivocado no sirve ni para reanudar.
      await rm(partPath, { force: true }).catch(() => undefined)
      return fail(
        'SU-E111',
        `el archivo descargado no coincide con el manifiesto (esperado ${request.sha256.slice(0, 12)}…, obtenido ${actual.slice(0, 12)}…)`,
      )
    }

    try {
      // `rename` dentro del mismo directorio es atómico: nadie puede observar un
      // modelo a medio escribir con el nombre definitivo.
      await rename(partPath, request.destPath)
    } catch (error) {
      return fail('SU-E150', `no se pudo colocar el modelo en su sitio: ${describe(error)}`)
    }

    const bytes = await stat(request.destPath).then(
      (info) => info.size,
      () => attempt.receivedBytes,
    )
    return { status: 'completed', bytes }
  }

  if (request.signal?.aborted) return { status: 'cancelled' }
  return fail('SU-E112', lastMessage)
}

type MirrorAttempt =
  | { status: 'completed'; receivedBytes: number }
  | { status: 'cancelled' }
  | { status: 'failed'; message: string }

/**
 * El resultado de una petición, que añade un caso al de un intento.
 *
 * `stale` no es un fallo: es "el `.part` que hay no vale, empieza de cero". Solo
 * [`attemptMirror`] puede decidir qué hacer con eso, así que no sale de ahí.
 */
type FetchOutcome = MirrorAttempt | { status: 'stale' }

/**
 * Un intento contra un espejo concreto.
 *
 * Reanuda si hay un `.part` de una descarga anterior: un modelo de 340 MB no se
 * vuelve a empezar desde cero porque se cortara la conexión a mitad.
 */
async function attemptMirror(
  request: DownloadRequest,
  url: string,
  partPath: string,
): Promise<MirrorAttempt> {
  const already = await stat(partPath).then(
    (info) => info.size,
    () => 0,
  )

  if (already === 0) {
    return withoutStale(await fetchInto(request, url, partPath, 0))
  }

  const resumed = await fetchInto(request, url, partPath, already)
  if (resumed.status !== 'stale') return resumed

  // El parcial no le sirve al servidor (el archivo remoto encogió). Se borra y
  // se pide entero: un archivo a medias obsoleto no debería obligar al usuario a
  // buscar otro espejo ni a pulsar descargar dos veces.
  await rm(partPath, { force: true }).catch(() => undefined)
  return withoutStale(await fetchInto(request, url, partPath, 0))
}

/**
 * Un `stale` nunca sale de aquí: significa "vuelve a empezar", no "esto ha
 * fallado", y quien decide eso es [`attemptMirror`]. Si se llegara a escapar
 * sería un fallo del espejo, que es lo que se devuelve.
 */
function withoutStale(outcome: FetchOutcome): MirrorAttempt {
  if (outcome.status === 'stale') {
    return { status: 'failed', message: 'el servidor rechazó reanudar la descarga (HTTP 416)' }
  }
  return outcome
}

/** Una petición. `from` es el byte por el que empezar, 0 para el archivo entero. */
async function fetchInto(
  request: DownloadRequest,
  url: string,
  partPath: string,
  from: number,
): Promise<FetchOutcome> {
  const already = from

  const controller = new AbortController()
  const onUserAbort = () => controller.abort()
  request.signal?.addEventListener('abort', onUserAbort, { once: true })

  // Distingue "el usuario canceló" de "esto se ha quedado muerto": las dos cosas
  // abortan, pero significan lo contrario.
  let stalled = false
  const stallMs = request.stallMs ?? DEFAULT_STALL_MS
  let stallTimer: NodeJS.Timeout | undefined

  const armStallTimer = () => {
    clearTimeout(stallTimer)
    stallTimer = setTimeout(() => {
      stalled = true
      controller.abort()
    }, stallMs)
  }

  const startedAt = Date.now()
  let received = already
  let lastReportAt = 0
  let total = request.sizeBytes

  try {
    const headers: Record<string, string> = {}
    if (already > 0) headers['range'] = `bytes=${already}-`

    // El temporizador cubre también la petición, no solo el cuerpo. Un servidor
    // que acepta la conexión y no contesta nunca deja `fetch` esperando para
    // siempre: sin esto, la descarga se quedaría ahí sin decir nada, que es la
    // peor forma de fallar.
    armStallTimer()
    const response = await fetch(url, { headers, signal: controller.signal, redirect: 'follow' })

    if (!response.ok) {
      // Un 416 significa que el `.part` es más grande que el archivo remoto:
      // está inservible.
      if (response.status === 416 && already > 0) {
        return { status: 'stale' }
      }
      return { status: 'failed', message: `HTTP ${response.status}` }
    }

    // El servidor puede ignorar el `Range` y mandar el archivo entero: en ese
    // caso hay que empezar de cero, no añadir al final.
    const resuming = already > 0 && response.status === 206
    if (already > 0 && !resuming) received = 0

    const contentLength = Number(response.headers.get('content-length') ?? 0)
    if (total === 0) total = received + (Number.isFinite(contentLength) ? contentLength : 0)

    if (!response.body) {
      return { status: 'failed', message: 'la respuesta no trae cuerpo' }
    }

    const stream = createWriteStream(partPath, { flags: resuming ? 'a' : 'w' })
    const reader = response.body.getReader()

    try {
      for (;;) {
        armStallTimer()
        const { done, value } = await reader.read()
        clearTimeout(stallTimer)

        if (done) break
        if (!value) continue

        received += value.byteLength
        await writeChunk(stream, value)

        const now = Date.now()
        const progressMs = request.progressMs ?? DEFAULT_PROGRESS_MS
        const isLast = total > 0 && received >= total
        if (progressMs === 0 || isLast || now - lastReportAt >= progressMs) {
          lastReportAt = now
          request.onProgress?.(progress(received, total, url, startedAt))
        }
      }
    } finally {
      clearTimeout(stallTimer)
      // Se suelta el cuerpo explícitamente. Si no, tras un abort la conexión
      // queda colgando del pool de `fetch` y el proceso no llega a cerrarse
      // aunque la descarga ya haya terminado.
      await reader.cancel().catch(() => undefined)
      await closeStream(stream)
    }

    // Si el archivo quedó corto, no se da por bueno: se guarda para reanudar en
    // el siguiente intento en lugar de fingir que terminó.
    if (total > 0 && received < total) {
      return { status: 'failed', message: `descarga incompleta (${received} de ${total} bytes)` }
    }

    return { status: 'completed', receivedBytes: received }
  } catch (error) {
    if (stalled) {
      return { status: 'failed', message: `sin datos durante ${stallMs} ms` }
    }
    if (request.signal?.aborted) return { status: 'cancelled' }
    if (isAbort(error)) return { status: 'cancelled' }
    return { status: 'failed', message: describe(error) }
  } finally {
    clearTimeout(stallTimer)
    request.signal?.removeEventListener('abort', onUserAbort)
  }
}

function progress(
  receivedBytes: number,
  totalBytes: number,
  url: string,
  startedAt: number,
): DownloadProgress {
  const elapsed = (Date.now() - startedAt) / 1000
  return {
    receivedBytes,
    totalBytes,
    url,
    ratio: totalBytes > 0 ? Math.min(1, receivedBytes / totalBytes) : null,
    bytesPerSecond: elapsed > 0.25 ? receivedBytes / elapsed : null,
  }
}

/** Espera a que el trozo se escriba de verdad, para no acumular en memoria. */
function writeChunk(stream: ReturnType<typeof createWriteStream>, chunk: Uint8Array): Promise<void> {
  return new Promise((resolve, reject) => {
    stream.write(chunk, (error) => (error ? reject(error) : resolve()))
  })
}

function closeStream(stream: ReturnType<typeof createWriteStream>): Promise<void> {
  return new Promise((resolve, reject) => {
    stream.end((error?: Error | null) => (error ? reject(error) : resolve()))
  })
}

/** `sha256` del archivo, en hexadecimal minúscula. */
export async function sha256File(path: string): Promise<string> {
  const handle = await open(path, 'r')
  const hash = createHash('sha256')
  try {
    const buffer = Buffer.allocUnsafe(1024 * 1024)
    for (;;) {
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, null)
      if (bytesRead === 0) break
      hash.update(buffer.subarray(0, bytesRead))
    }
  } finally {
    await handle.close()
  }
  return hash.digest('hex')
}

function isAbort(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'name' in error &&
    (error as { name?: unknown }).name === 'AbortError'
  )
}

function describe(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}

function fail(errorCode: ErrorCode, message: string): DownloadResult {
  return { status: 'failed', errorCode, message }
}
