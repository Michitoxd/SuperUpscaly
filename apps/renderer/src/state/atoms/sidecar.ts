import { atom } from 'jotai'
import type {
  SidecarCapabilities,
  SidecarJob,
  SidecarModelStatus,
  SidecarPipeline,
  SidecarStatus,
} from '@superupscaly/shared'

/**
 * Estado del motor de escalado tal como lo ve la interfaz.
 *
 * El estado del sidecar **no se duplica**: es una proyeccion de lo que informa el
 * proceso principal, que a su vez lo lee del proceso real. La interfaz nunca
 * asume que esta listo; pregunta.
 */

export const sidecarStatusAtom = atom<SidecarStatus>({
  state: 'stopped',
  restarts: 0,
  missingBinary: false,
})

export const sidecarReadyAtom = atom((get) => get(sidecarStatusAtom).state === 'ready')

export const capabilitiesAtom = atom<SidecarCapabilities | null>(null)

export const modelsAtom = atom<SidecarModelStatus[]>([])

/**
 * Los pipelines que el motor va a usar.
 *
 * Se preguntan al sidecar en lugar de reconstruirlos aqui: el panel avanzado los
 * dibuja, y una copia en cliente de las reglas del motor es una lista que acaba
 * diciendo algo distinto de lo que se ejecuta.
 */
export const pipelinesAtom = atom<SidecarPipeline[]>([])

/**
 * Directorio donde el sidecar busca los modelos, tal como lo resolvio el.
 *
 * No se calcula en la interfaz a proposito: si la interfaz creyera que los
 * modelos viven en un sitio y el motor los buscara en otro, la lista diria
 * "instalado" mientras el motor sigue sin encontrarlos.
 */
export const modelsDirAtom = atom<string>('')

/** Trabajo en curso, si lo hay. */
export const activeJobAtom = atom<SidecarJob | null>(null)

/** Historial de trabajos, del mas reciente al mas antiguo. */
export const jobHistoryAtom = atom<SidecarJob[]>([])
