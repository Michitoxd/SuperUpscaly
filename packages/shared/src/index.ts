/**
 * Extension explicita en cada importacion relativa, y no es un capricho de estilo:
 * el sidecar no es el unico consumidor de este paquete, y las pruebas del proceso
 * principal cargan TypeScript directamente con `node --test` (Node borra los tipos
 * sin compilar). Node resuelve un `./types` sin extension solo si hay un paso de
 * compilacion que lo complete, y ahi no lo hay.
 *
 * Con la extension, `import { isSafePathString } from '@superupscaly/shared'`
 * funciona tal cual tanto en el paquete empaquetado por esbuild como en una prueba
 * suelta. Se paga una linea mas larga por poder probar el codigo compartido de
 * verdad, en lugar de probar una copia.
 */
export * from './types.ts'
export * from './media.ts'
export * from './sidecar.ts'
export * from './api.ts'
export * from './theme.ts'
export * from './error-codes.ts'
export * from './guards.ts'
export * from './i18n/index.ts'
export type { TranslationKey, Translator } from './i18n/index.ts'
