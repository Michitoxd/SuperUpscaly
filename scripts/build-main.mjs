#!/usr/bin/env node
/**
 * Empaqueta el proceso principal y el preload con esbuild.
 *
 * Se usa esbuild en lugar de tsc porque resuelve los paquetes TypeScript del
 * monorepo (`@superupscaly/shared`) sin necesidad de compilarlos aparte ni de
 * configurar `rootDir`/`paths` para cada combinacion de salida.
 */
import { build } from 'esbuild'
import { mkdirSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const desktopDir = join(root, 'apps', 'desktop')
const outDir = join(desktopDir, 'dist')

const shared = {
  bundle: true,
  platform: 'node',
  target: 'node20',
  format: 'cjs',
  sourcemap: true,
  // `electron` solo existe dentro del runtime de Electron: nunca se empaqueta.
  // `ws` se deja fuera porque trae dos dependencias nativas opcionales
  // (`bufferutil` y `utf-8-validate`) que esbuild intentaria resolver y que no
  // estan instaladas: en tiempo de ejecucion `ws` las busca y sigue sin ellas.
  external: ['electron', 'ws', 'bufferutil', 'utf-8-validate'],
  logLevel: 'info',
  minify: false,
}

// La limpieza es una comodidad, no un requisito: esbuild sobrescribe la salida de
// todos modos. Se tolera el fallo porque en Windows es normal que un archivo
// quede bloqueado por un Electron que acaba de cerrarse, y un build no deberia
// morir por eso.
try {
  rmSync(outDir, { recursive: true, force: true })
} catch (error) {
  console.warn(
    `[build-main] aviso: no se pudo limpiar ${outDir} (${error.code ?? error.message}); se continua`,
  )
}

mkdirSync(join(outDir, 'main'), { recursive: true })
mkdirSync(join(outDir, 'preload'), { recursive: true })

await build({
  ...shared,
  entryPoints: [join(desktopDir, 'src', 'main', 'index.ts')],
  outfile: join(outDir, 'main', 'index.js'),
})

await build({
  ...shared,
  entryPoints: [join(desktopDir, 'src', 'preload', 'index.ts')],
  outfile: join(outDir, 'preload', 'index.js'),
})

console.log('[build-main] listo -> apps/desktop/dist')
