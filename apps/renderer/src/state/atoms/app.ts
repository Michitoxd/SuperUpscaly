import { atom } from 'jotai'
import type { AppInfo } from '@superupscaly/shared'
import { getBridge } from '@/lib/bridge'
import { hydrateSettingsAtom, settingsAtom } from './settings'

export const appInfoAtom = atom<AppInfo | null>(null)

/** true cuando ya se intento cargar todo el estado inicial. */
export const bootstrappedAtom = atom(false)

/** Carpeta de salida por defecto del sistema. */
export const defaultOutputDirAtom = atom('')

/** Carpeta de salida efectiva (la elegida por el usuario o la del sistema). */
export const outputDirAtom = atom('')

export type OutputDirStatus = 'unknown' | 'ok' | 'invalid'
export const outputDirStatusAtom = atom<OutputDirStatus>('unknown')

/**
 * Carga inicial: ajustes, informacion de la app y carpeta de salida.
 *
 * Todo lo que puede fallar se captura aqui para que la UI arranque siempre en un
 * estado coherente, incluso si el proceso principal no responde.
 */
export const bootstrapAtom = atom(null, async (get, set): Promise<void> => {
  await set(hydrateSettingsAtom)

  const bridge = getBridge()
  if (!bridge) {
    set(bootstrappedAtom, true)
    return
  }

  try {
    const [info, systemDefault] = await Promise.all([
      bridge.getAppInfo(),
      bridge.getDefaultOutputDir(),
    ])
    set(appInfoAtom, info)
    set(defaultOutputDirAtom, systemDefault)

    const configured = get(settingsAtom).outputDir
    const target = configured ?? systemDefault
    const ensured = await bridge.ensureDir(target)

    set(outputDirAtom, ensured.ok ? ensured.path : target)
    set(outputDirStatusAtom, ensured.ok ? 'ok' : 'invalid')
  } catch {
    set(outputDirStatusAtom, 'invalid')
  }

  set(bootstrappedAtom, true)
})
