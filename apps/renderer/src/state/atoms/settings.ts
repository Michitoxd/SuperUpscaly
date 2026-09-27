import { atom } from 'jotai'
import { createTranslator, type Translator } from '@superupscaly/shared'
import { DEFAULT_SETTINGS, type AppSettings } from '@superupscaly/shared'
import { getBridge } from '@/lib/bridge'

/** Ajustes activos. Es la copia local optimista de `<appData>/settings.json`. */
export const settingsAtom = atom<AppSettings>(DEFAULT_SETTINGS)

/** true cuando los ajustes reales ya se leyeron del disco. */
export const settingsReadyAtom = atom(false)

export const localeAtom = atom((get) => get(settingsAtom).locale)

/** Traductor ligado al idioma activo. Los componentes hacen `const t = useAtomValue(translatorAtom)`. */
export const translatorAtom = atom<Translator>((get) => createTranslator(get(settingsAtom).locale))

/**
 * Aplica un parche de ajustes.
 *
 * Se actualiza el atom primero (la UI responde al instante) y despues se
 * persiste. Si el proceso principal normaliza o descarta algun campo, la
 * respuesta del disco manda y sobrescribe la copia optimista: la UI nunca
 * muestra un ajuste que no se haya guardado de verdad.
 */
export const updateSettingsAtom = atom(
  null,
  async (get, set, patch: Partial<AppSettings>): Promise<void> => {
    set(settingsAtom, { ...get(settingsAtom), ...patch })

    const bridge = getBridge()
    if (!bridge) return

    try {
      const persisted = await bridge.setSettings(patch)
      set(settingsAtom, persisted)
    } catch {
      // Si falla el guardado, recargamos el estado real para no mentir al usuario.
      try {
        set(settingsAtom, await bridge.getSettings())
      } catch {
        // Sin puente disponible no hay nada mas que hacer.
      }
    }
  },
)

/** Carga los ajustes persistidos. Se llama una vez al arrancar la app. */
export const hydrateSettingsAtom = atom(null, async (_get, set): Promise<void> => {
  const bridge = getBridge()
  if (!bridge) {
    set(settingsReadyAtom, true)
    return
  }
  try {
    set(settingsAtom, await bridge.getSettings())
  } catch {
    // Se mantienen los valores por defecto.
  }
  set(settingsReadyAtom, true)
})
