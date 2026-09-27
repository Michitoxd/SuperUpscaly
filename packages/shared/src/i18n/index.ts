import { es, type Dictionary } from './es.ts'
import { en } from './en.ts'
import type { Locale } from '../types.ts'
import type { ErrorCode } from '../error-codes.ts'

export { es, en }
export type { Dictionary }

export const dictionaries: Record<Locale, Dictionary> = { es, en }

/**
 * El catalogo de errores y el diccionario tienen que crecer juntos.
 *
 * Si anades un `SU-Exxx` a `error-codes.ts` y olvidas su texto, la UI mostraria
 * la clave cruda (`errors.SU-Exxx.message`) en lugar de una frase: un fallo
 * silencioso, que es justo lo que este proyecto prohibe. Esta asignacion
 * convierte ese olvido en un error de compilacion.
 *
 * Basta con comprobarlo en espanol: `en` esta tipado como `Dictionary`, que se
 * deriva de `es`, asi que le faltan exactamente las mismas claves.
 */
const _todo_codigo_de_error_tiene_texto: Record<
  ErrorCode,
  { message: string; action: string }
> = es.errors

export const LOCALES: readonly Locale[] = ['es', 'en']

export function isLocale(value: unknown): value is Locale {
  return value === 'es' || value === 'en'
}

/** Rutas de clave hoja, p. ej. `'queue.empty' | 'errors.SU-E130.message' | …`. */
type LeafPaths<T> = {
  [K in keyof T & string]: T[K] extends string ? K : `${K}.${LeafPaths<T[K]>}`
}[keyof T & string]

export type TranslationKey = LeafPaths<Dictionary>

function lookup(dict: Dictionary, key: string): string | undefined {
  let node: unknown = dict
  for (const part of key.split('.')) {
    if (typeof node !== 'object' || node === null) return undefined
    node = (node as Record<string, unknown>)[part]
  }
  return typeof node === 'string' ? node : undefined
}

/** Sustituye `{placeholder}` por el valor correspondiente. */
export function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = vars[name]
    return value === undefined ? match : String(value)
  })
}

/**
 * Traduce una clave. Si falta en el idioma activo se cae al espanol y, si
 * tampoco existe ahi, se devuelve la propia clave en lugar de una cadena vacia
 * (un hueco visible es un bug; una cadena vacia es un bug invisible).
 */
export function translate(
  locale: Locale,
  key: TranslationKey,
  vars?: Record<string, string | number>,
): string {
  const raw = lookup(dictionaries[locale], key) ?? lookup(dictionaries.es, key) ?? key
  return interpolate(raw, vars)
}

export type Translator = (key: TranslationKey, vars?: Record<string, string | number>) => string

export function createTranslator(locale: Locale): Translator {
  return (key, vars) => translate(locale, key, vars)
}
