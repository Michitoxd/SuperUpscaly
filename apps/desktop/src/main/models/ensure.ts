/**
 * Garantiza que el motor tenga el modelo que va a necesitar antes de aceptar el
 * trabajo.
 *
 * ## Por que existe
 *
 * Un equipo recien instalado tiene ONNX Runtime (viaja con la aplicacion) y el
 * directorio de modelos **vacio**. En ese estado el motor no puede cargar la
 * etapa de escalado, y la primera imagen que el usuario prueba se pierde: la
 * aplicacion arranca bien, deja arrastrar una imagen, y el resultado es un fallo.
 * Es el peor orden posible de descubrimiento.
 *
 * La documentacion del proyecto ya decia que los modelos «se descargan en el
 * primer uso»; esto es lo que faltaba para que fuera cierto. El catalogo declara
 * URL, tamano y hash de cada modelo, asi que la descarga se puede comprobar pieza
 * a pieza, que es la unica forma de que instalar un peso de terceros sea algo
 * distinto de creerle a la red.
 *
 * ## Que decide aqui y que no
 *
 * - **Que modelo hace falta lo dice el motor**, no esta funcion: se leen los
 *   pipelines de `/v1/pipelines`. Cuál es el modelo de una etapa es una decision
 *   del sidecar, y duplicarla en TypeScript seria garantizar que las dos copias se
 *   separen (fue exactamente lo que paso con el plan que dibuja la interfaz).
 * - **Solo se descarga lo imprescindible**: las etapas que **escalan**. Las de
 *   restauracion (limpieza de lineas, rostro) ya se omiten solas con su motivo
 *   cuando falta su modelo, asi que descargarlas antes de tiempo gastaria cientos
 *   de megabytes por nada.
 * - **Un fallo de descarga no cancela el trabajo.** Se anota y se sigue: el motor
 *   tiene respaldo y el informe dice con que se hizo. Perder la imagen porque el
 *   wifi va mal seria peor que darle un resultado interpolado — y decirlo.
 *
 * El modulo no escribe en el registro (ni importa nada de Electron): devuelve un
 * informe y **quien lo llama decide que contar**. Asi se puede probar con
 * `node --test`, que es como se prueba el resto del proceso principal.
 */

import type {
  ModelDownloadEvent,
  SidecarCapabilities,
  SidecarModelStatus,
  SidecarModelsResult,
  SidecarPipeline,
  SidecarPipelineStage,
  SidecarPipelinesResult,
} from '@superupscaly/shared'

/** Lo que hace falta saber preguntarle al sidecar. */
export interface ModelCatalogSource {
  capabilities(): Promise<SidecarCapabilities>
  models(): Promise<SidecarModelsResult>
  pipelines(): Promise<SidecarPipelinesResult>
}

export interface EnsureRequest {
  mode: 'photo' | 'illustration'
  scale: number
  /** Ajustes que cambian que modelo se usa. */
  modelChainMode: 'auto' | 'manual'
  upscaleModel?: string | null
}

export interface EnsureReport {
  /** Modelos que ya estaban instalados. */
  present: string[]
  /** Modelos que se descargaron en esta llamada. */
  downloaded: string[]
  /** Modelos imprescindibles que no se pudieron conseguir, con el motivo. */
  unavailable: Array<{ modelId: string; reason: string }>
  /** `true` cuando no habia nada que hacer porque el motor no usa modelos. */
  interpolated: boolean
  /** Motivo por el que no se pudo consultar el catalogo, si fue el caso. */
  catalogError: string | null
}

/** Escala nativa declarada por el catalogo para un modelo. */
function nativeScaleOf(catalog: readonly SidecarModelStatus[], modelId: string): number | undefined {
  return catalog.find((entry) => entry.id === modelId)?.scale
}

/**
 * `true` si la etapa amplia la imagen.
 *
 * Es la misma cuenta que hace el runner: `scaleOut` manda y, si no esta, la escala
 * la pone el modelo. Una etapa que no amplia (restauracion, analisis) no obliga a
 * descargar nada: cuando falta su modelo se omite y el propio informe lo dice.
 */
function stageScales(
  stage: SidecarPipelineStage,
  catalog: readonly SidecarModelStatus[],
): boolean {
  if (stage.op !== 'model' || !stage.model) return false
  const scale = stage.scaleOut ?? nativeScaleOf(catalog, stage.model) ?? 1
  return scale > 1
}

/**
 * Modelos **imprescindibles** para un modo y una escala.
 *
 * Devuelve una lista sin repetidos y en el orden en el que el motor los va a
 * necesitar. Si no hay pipeline para esa combinacion devuelve una lista vacia: no
 * saber que hace falta no es motivo para descargar algo, y el trabajo lo rechazara
 * el sidecar con el motivo escrito.
 */
export function requiredModels(
  pipelines: readonly SidecarPipeline[],
  catalog: readonly SidecarModelStatus[],
  request: EnsureRequest,
): string[] {
  const pipeline = pipelines.find(
    (entry) => entry.mode === request.mode && entry.scale === request.scale,
  )

  if (!pipeline) return []

  // El modo Manual sustituye el modelo de las etapas que escalan (lo mismo que
  // hace `model_for_stage` en el runner). Si el usuario eligio un modelo, es **ese**
  // el que hay que tener.
  const manual =
    request.modelChainMode === 'manual' && request.upscaleModel && request.upscaleModel.length > 0
      ? request.upscaleModel
      : null

  const needed: string[] = []
  for (const stage of pipeline.stages) {
    if (!stageScales(stage, catalog)) continue

    const model = manual ?? stage.model!
    if (!needed.includes(model)) needed.push(model)
  }

  return needed
}

/** `true` si el motor de este arranque puede usar modelos. */
export function engineUsesModels(engine: string): boolean {
  // El motor clasico se anuncia como `clasico-<filtro>`. Cuando es el que corre,
  // descargar un modelo no cambiaria nada: el sidecar no sabe cargarlo.
  return !engine.toLowerCase().startsWith('clasico')
}

/**
 * Se asegura de que los modelos imprescindibles esten descargados.
 *
 * Nunca lanza: devuelve lo que consiguio y lo que no. Un fallo de red, un catalogo
 * que no responde o un hash que no cuadra son resultados que la interfaz tiene que
 * poder contar, no excepciones que corten la creacion del trabajo.
 */
export async function ensureModelsFor(
  source: ModelCatalogSource,
  request: EnsureRequest,
  download: (modelId: string) => Promise<ModelDownloadEvent>,
): Promise<EnsureReport> {
  const report: EnsureReport = {
    present: [],
    downloaded: [],
    unavailable: [],
    interpolated: false,
    catalogError: null,
  }

  let capabilities: SidecarCapabilities
  let catalog: SidecarModelsResult
  let pipelines: SidecarPipelinesResult

  try {
    capabilities = await source.capabilities()
    catalog = await source.models()
    pipelines = await source.pipelines()
  } catch (error) {
    // Sin catalogo no se puede comprobar ni descargar nada. Se deja pasar el
    // trabajo: el sidecar respondera con el error que corresponda y con su codigo.
    return { ...report, catalogError: describe(error) }
  }

  if (!engineUsesModels(capabilities.engine)) {
    report.interpolated = true
    return report
  }

  const needed = requiredModels(pipelines.pipelines, catalog.models, request)

  for (const modelId of needed) {
    const entry = catalog.models.find((model) => model.id === modelId)

    if (!entry) {
      report.unavailable.push({ modelId, reason: 'el catalogo no conoce este modelo' })
      continue
    }

    if (entry.state === 'installed') {
      report.present.push(modelId)
      continue
    }

    const urls = entry.download?.urls ?? []
    if (urls.length === 0) {
      // Un modelo sin origen declarado no se puede traer: hay que instalarlo a
      // mano. Se dice con esas palabras en lugar de intentar una descarga vacia.
      report.unavailable.push({
        modelId,
        reason: 'el catalogo no declara de donde descargarlo',
      })
      continue
    }

    const result = await download(modelId)

    if (result.status === 'completed') {
      report.downloaded.push(modelId)
    } else {
      report.unavailable.push({
        modelId,
        reason: result.errorCode ?? result.status,
      })
    }
  }

  return report
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
