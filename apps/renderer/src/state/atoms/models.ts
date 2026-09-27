import { atom } from 'jotai'
import type { ModelDownloadEvent, TranslationKey } from '@superupscaly/shared'
import { getBridge } from '@/lib/bridge'
import { modelsAtom, modelsDirAtom, pipelinesAtom } from './sidecar'
import { translatorAtom } from './settings'
import { pushToastAtom } from './ui'

/**
 * Estado de la descarga de modelos.
 *
 * La interfaz no deduce el estado de un modelo de la descarga: lo **pregunta**.
 * El sidecar comprueba el archivo y su hash, y es el unico que puede afirmar que
 * un modelo esta instalado. Dar por bueno lo que acaba de escribirse en disco
 * seria creerle a la descarga en lugar de al archivo.
 */

/** Si el panel de modelos esta abierto. */
export const modelsOpenAtom = atom(false)

/** Ultimo evento recibido de cada descarga, por `id` de modelo. */
export const downloadsAtom = atom<Record<string, ModelDownloadEvent>>({})

/** Vuelve a preguntar el catalogo y los pipelines al sidecar. */
export const refreshModelsAtom = atom(null, async (_get, set): Promise<void> => {
  const bridge = getBridge()
  if (!bridge) return

  try {
    const catalog = await bridge.sidecarModels()
    set(modelsDirAtom, catalog.modelsDir)
    set(modelsAtom, catalog.models)
  } catch {
    // El sidecar puede no estar listo todavia. No se vacia la lista: lo que ya
    // se sabia sigue siendo cierto, y vaciarla haria parpadear la interfaz cada
    // vez que el motor se reinicia.
  }

  // Los pipelines van en la misma vuelta porque los piden los mismos momentos (el
  // arranque del motor y el fin de una descarga) y el panel que los dibuja se
  // equivoca si le llegan los modelos nuevos con la cadena vieja.
  try {
    const pipelines = await bridge.sidecarPipelines()
    set(pipelinesAtom, pipelines.pipelines)
  } catch {
    // Igual que arriba: lo que ya se sabia sigue valiendo.
  }
})

/** Descarga un modelo y refresca el catalogo con lo que diga el sidecar. */
export const startDownloadAtom = atom(null, async (get, set, modelId: string): Promise<void> => {
  const bridge = getBridge()
  if (!bridge) return

  const t = get(translatorAtom)
  const result = await bridge.downloadModel(modelId)

  set(downloadsAtom, { ...get(downloadsAtom), [modelId]: result })

  if (result.status === 'failed' && result.errorCode) {
    set(pushToastAtom, {
      tone: 'error',
      code: result.errorCode,
      message: t(`errors.${result.errorCode}.message` as TranslationKey),
      action: t(`errors.${result.errorCode}.action` as TranslationKey),
    })
  }

  await set(refreshModelsAtom)
})

export const cancelDownloadAtom = atom(null, async (_get, _set, modelId: string): Promise<void> => {
  const bridge = getBridge()
  if (bridge) await bridge.cancelModelDownload(modelId)
})

/** Aplica un evento de progreso que llega del proceso principal. */
export const applyDownloadEventAtom = atom(
  null,
  (get, set, event: ModelDownloadEvent): void => {
    set(downloadsAtom, { ...get(downloadsAtom), [event.modelId]: event })
  },
)
