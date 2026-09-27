import type { SidecarPipeline, SidecarPipelineStage, StageId } from '@superupscaly/shared'

export interface ChainStage {
  /** Etapa tal como la nombra la interfaz. */
  stage: StageId
  /** Modelo que interviene, si la etapa usa uno. */
  model?: string
  /**
   * `true` si la etapa puede no ejecutarse según el análisis de la imagen.
   *
   * Es lo único que la interfaz puede afirmar sin analizar la imagen: el motor
   * decide cada condición con sus propios números (ruido, artefactos, rostros) y
   * publica el resultado por imagen en `effectivePipeline` y en las notas. Marcar
   * las condicionales es honesto; prometer que se van a omitir o a ejecutar, no.
   */
  conditional: boolean
}

/**
 * Etapas que la interfaz dibuja a partir de un pipeline **real del motor**.
 *
 * ## Por qué ya no se calcula aquí
 *
 * Antes esta función reproducía en TypeScript la lógica del motor: elegía el
 * modelo por modo con dos identificadores escritos a mano y dibujaba las etapas a
 * partir de los ajustes. Esa copia se quedó atrás en cuanto hubo más de una escala
 * —a 2x el motor usa `2x-animesharpv3` y el panel anunciaba el modelo de 4x— y
 * enseñaba etapas que el análisis puede omitir. Dos listas de lo mismo acaban
 * diciendo cosas distintas; ahora hay una sola, la del motor, y esto solo la
 * proyecta al vocabulario de la interfaz.
 *
 * ## Las etapas de entrada y salida
 *
 * `decode` y `encode` no están en el pipeline porque no son decisiones: la
 * aplicación siempre decodifica antes y escribe después. Se añaden para que el
 * usuario vea la cadena completa, que es lo que la sección promete.
 */
export function planStages(pipeline: SidecarPipeline | undefined): ChainStage[] {
  const stages: ChainStage[] = [{ stage: 'decode', conditional: false }]

  if (pipeline) {
    for (const entry of pipeline.stages) {
      const mapped = toStageId(entry)
      // Una etapa que la interfaz no sabe nombrar se omite en lugar de dibujarse
      // con el identificador del motor, que no significa nada para el usuario.
      if (!mapped) continue
      stages.push({
        stage: mapped,
        ...(entry.model ? { model: entry.model } : {}),
        conditional: isConditional(entry),
      })
    }
  }

  stages.push({ stage: 'encode', conditional: false })
  return stages
}

/**
 * Traduce el identificador de una etapa del motor al vocabulario de la interfaz.
 *
 * Varias etapas del motor caen en el mismo rótulo a propósito: `downscale`,
 * `halve` y `upscale2` son formas de escalar, y la interfaz solo tiene esa
 * palabra. Devolver `null` es una respuesta válida para lo que no encaja.
 */
function toStageId(stage: SidecarPipelineStage): StageId | null {
  switch (stage.op) {
    case 'analyze':
      return 'analyze'
    case 'model':
      // Los pipelines usan identificadores propios (`lineclean` limpia líneas,
      // `face` restaura rostros). El modelo dice a qué familia pertenece.
      if (stage.id.includes('face')) return 'face'
      if (stage.id.includes('clean') || stage.id.includes('denoise')) return 'denoise'
      return 'upscale'
    case 'unsharp':
      return 'sharpen'
    case 'resize':
      return 'upscale'
    default:
      return null
  }
}

/** `true` si la etapa declara una condición, sea cual sea. */
function isConditional(stage: SidecarPipelineStage): boolean {
  return stage.when !== undefined
}

/** El pipeline que corresponde a un modo y una escala, si el motor lo tiene. */
export function pipelineFor(
  pipelines: readonly SidecarPipeline[] | undefined,
  mode: 'photo' | 'illustration',
  scale: number,
): SidecarPipeline | undefined {
  return pipelines?.find((entry) => entry.mode === mode && entry.scale === scale)
}
