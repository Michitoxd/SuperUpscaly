/**
 * Pruebas de la garantia de modelos.
 *
 * Lo que se prueba no es «descarga un archivo»: eso ya lo cubre
 * `downloader.test.ts`. Lo que se prueba es **la decision**: que modelo hace falta
 * para un modo y una escala, y que se hace cuando no se puede conseguir. Un error
 * aqui no da un fallo rojo: da una descarga de 68 MB que no hacia falta, o una
 * imagen interpolada que nadie ha pedido.
 */

import assert from 'node:assert/strict'
import { test } from 'node:test'

import type {
  ModelDownloadEvent,
  SidecarCapabilities,
  SidecarModelStatus,
  SidecarModelsResult,
  SidecarPipeline,
  SidecarPipelinesResult,
} from '@superupscaly/shared'

import { engineUsesModels, ensureModelsFor, requiredModels, type ModelCatalogSource } from './ensure.ts'

// --- Utilidades --------------------------------------------------------------

function model(
  id: string,
  scale: number,
  extras: Partial<SidecarModelStatus> = {},
): SidecarModelStatus {
  return {
    id,
    name: id,
    kind: 'illustration',
    scale,
    state: 'missing',
    path: `/modelos/${id}.onnx`,
    sizeBytes: 1024,
    // Por defecto, descargable: es lo que declara el manifiesto real de los cuatro
    // modelos que la aplicacion ofrece.
    download: {
      urls: [`https://ejemplo.invalido/${id}.onnx`],
      sha256: 'a'.repeat(64),
      sizeBytes: 1024,
      fileName: `${id}.onnx`,
    },
    ...extras,
  }
}

/** Los pipelines reales de `pipelines.default.json`, recortados a lo que importa. */
const PIPELINES: SidecarPipeline[] = [
  {
    id: 'illustration:4x',
    mode: 'illustration',
    scale: 4,
    stages: [
      { id: 'analyze', op: 'analyze' },
      { id: 'lineclean', op: 'model', model: 'realesrgan-x4plus-anime-6b', scaleOut: 1 },
      { id: 'upscale', op: 'model', model: 'realesrgan-x4plus-anime-6b', scaleOut: 4 },
      { id: 'sharpen', op: 'unsharp' },
    ],
  },
  {
    id: 'illustration:8x',
    mode: 'illustration',
    scale: 8,
    stages: [
      { id: 'analyze', op: 'analyze' },
      { id: 'upscale', op: 'model', model: 'realesrgan-x4plus-anime-6b', scaleOut: 4 },
      { id: 'halve', op: 'resize' },
      { id: 'upscale2', op: 'model', model: 'realesrgan-x4plus-anime-6b', scaleOut: 4 },
    ],
  },
  {
    id: 'photo:4x',
    mode: 'photo',
    scale: 4,
    stages: [
      { id: 'analyze', op: 'analyze' },
      { id: 'denoise', op: 'model', model: 'scunet-color' },
      { id: 'upscale', op: 'model', model: '4x-ultrasharp', scaleOut: 4 },
      { id: 'face', op: 'model', model: 'gfpgan-v1.4' },
    ],
  },
]

const CATALOG: SidecarModelStatus[] = [
  model('realesrgan-x4plus-anime-6b', 4),
  model('4x-ultrasharp', 4),
  model('scunet-color', 1),
  model('gfpgan-v1.4', 1),
]

const AUTO = { modelChainMode: 'auto' as const, upscaleModel: null }

function completed(modelId: string): ModelDownloadEvent {
  return {
    modelId,
    status: 'completed',
    receivedBytes: 10,
    totalBytes: 10,
    ratio: 1,
    bytesPerSecond: null,
    url: 'https://ejemplo.invalido/modelo.onnx',
    errorCode: null,
  }
}

interface Harness {
  source: ModelCatalogSource
  calls: string[]
}

function harness(options: {
  models?: SidecarModelStatus[]
  pipelines?: SidecarPipeline[]
  engine?: string
  download?: (modelId: string) => Promise<ModelDownloadEvent>
  catalogFails?: boolean
}): Harness {
  const calls: string[] = []
  const models = options.models ?? CATALOG
  const pipelines = options.pipelines ?? PIPELINES

  const source: ModelCatalogSource = {
    async capabilities(): Promise<SidecarCapabilities> {
      if (options.catalogFails) throw new Error('sin sidecar')
      return { engine: options.engine ?? 'CUDA' } as SidecarCapabilities
    },
    async models(): Promise<SidecarModelsResult> {
      if (options.catalogFails) throw new Error('sin sidecar')
      return { modelsDir: '/modelos', models }
    },
    async pipelines(): Promise<SidecarPipelinesResult> {
      if (options.catalogFails) throw new Error('sin sidecar')
      return { version: 1, pipelines }
    },
  }

  return { source, calls }
}

// --- Que hace falta ---------------------------------------------------------

test('solo cuenta la etapa que escala, no la de restauracion', () => {
  // `lineclean` declara `scaleOut: 1`: cuando falta su modelo se omite y el
  // informe lo dice. Descargarlo antes de tiempo no cambiaria el resultado.
  const needed = requiredModels(PIPELINES, CATALOG, { mode: 'illustration', scale: 4, ...AUTO })
  assert.deepEqual(needed, ['realesrgan-x4plus-anime-6b'])
})

test('una etapa sin scaleOut cuenta por la escala que declara el modelo', () => {
  // Las de restauracion no declaran `scaleOut` porque devuelven el mismo tamano;
  // el catalogo dice que escalan 1, asi que no obligan a descargar nada.
  const needed = requiredModels(PIPELINES, CATALOG, { mode: 'photo', scale: 4, ...AUTO })
  assert.deepEqual(needed, ['4x-ultrasharp'])
})

test('las dos pasadas de 8x no piden el modelo dos veces', () => {
  const needed = requiredModels(PIPELINES, CATALOG, { mode: 'illustration', scale: 8, ...AUTO })
  assert.deepEqual(needed, ['realesrgan-x4plus-anime-6b'])
})

test('el modo Manual descarga el modelo que eligio el usuario', () => {
  // El runner sustituye el modelo de las etapas que escalan por el elegido a mano:
  // si aqui se pidiera el del pipeline, se descargaria el que no se va a usar.
  const needed = requiredModels(PIPELINES, CATALOG, {
    mode: 'illustration',
    scale: 4,
    modelChainMode: 'manual',
    upscaleModel: '4x-ultrasharp',
  })
  assert.deepEqual(needed, ['4x-ultrasharp'])
})

test('una combinacion sin pipeline no pide nada', () => {
  // No saber que hace falta no es motivo para descargar algo: el sidecar rechazara
  // el trabajo con su propio mensaje.
  const needed = requiredModels(PIPELINES, CATALOG, { mode: 'photo', scale: 8, ...AUTO })
  assert.deepEqual(needed, [])
})

test('el motor clasico no usa modelos', () => {
  assert.equal(engineUsesModels('clasico-catmullrom'), false)
  assert.equal(engineUsesModels('Clasico-Lanczos3'), false)
  assert.equal(engineUsesModels('CUDA'), true)
  assert.equal(engineUsesModels('CPU'), true)
})

// --- Que se hace cuando no se puede ----------------------------------------

test('se descarga lo que falta y se respeta lo instalado', async () => {
  const installed = CATALOG.map((entry) =>
    entry.id === 'realesrgan-x4plus-anime-6b' ? { ...entry, state: 'installed' as const } : entry,
  )
  const h = harness({ models: installed })
  const asked: string[] = []

  const report = await ensureModelsFor(h.source, { mode: 'illustration', scale: 4, ...AUTO }, async (id) => {
    asked.push(id)
    return completed(id)
  })

  assert.deepEqual(asked, [], 'no habia que descargar nada')
  assert.deepEqual(report.present, ['realesrgan-x4plus-anime-6b'])
  assert.deepEqual(report.downloaded, [])
})

test('un modelo sin URL no se intenta descargar y se dice por que', async () => {
  const h = harness({
    models: [model('realesrgan-x4plus-anime-6b', 4, { download: null })],
  })
  const asked: string[] = []

  const report = await ensureModelsFor(h.source, { mode: 'illustration', scale: 4, ...AUTO }, async (id) => {
    asked.push(id)
    return completed(id)
  })

  assert.deepEqual(asked, [], 'sin URL no hay descarga que intentar')
  assert.equal(report.unavailable.length, 1)
  assert.match(report.unavailable[0]!.reason, /de donde descargarlo/)
})

test('una descarga que falla no lanza: se anota con su codigo', async () => {
  const h = harness({})

  const report = await ensureModelsFor(h.source, { mode: 'illustration', scale: 4, ...AUTO }, async (id) => ({
    ...completed(id),
    status: 'failed',
    ratio: null,
    errorCode: 'SU-E112',
  }))

  assert.deepEqual(report.downloaded, [])
  assert.deepEqual(report.unavailable, [
    { modelId: 'realesrgan-x4plus-anime-6b', reason: 'SU-E112' },
  ])
})

test('sin catalogo no se inventa una descarga', async () => {
  // El sidecar puede estar reiniciandose. Lo que no puede pasar es que la
  // aplicacion descargue a ciegas o que la creacion del trabajo se caiga por esto.
  const h = harness({ catalogFails: true })
  let called = 0

  const report = await ensureModelsFor(h.source, { mode: 'illustration', scale: 4, ...AUTO }, async (id) => {
    called += 1
    return completed(id)
  })

  assert.equal(called, 0)
  assert.deepEqual(report.downloaded, [])
  assert.deepEqual(report.unavailable, [])
})

test('con el motor clasico no se descarga nada', async () => {
  const h = harness({ engine: 'clasico-catmullrom' })
  let called = 0

  const report = await ensureModelsFor(h.source, { mode: 'illustration', scale: 4, ...AUTO }, async (id) => {
    called += 1
    return completed(id)
  })

  assert.equal(called, 0, 'el sidecar no sabria cargar el modelo')
  assert.equal(report.interpolated, true)
})

test('se descarga exactamente el modelo que falta', async () => {
  const h = harness({ models: CATALOG })
  const asked: string[] = []

  const report = await ensureModelsFor(h.source, { mode: 'photo', scale: 4, ...AUTO }, async (id) => {
    asked.push(id)
    return completed(id)
  })

  assert.deepEqual(asked, ['4x-ultrasharp'])
  assert.deepEqual(report.downloaded, ['4x-ultrasharp'])
  assert.deepEqual(report.unavailable, [])
})
