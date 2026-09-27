#!/usr/bin/env node
/**
 * Ejecuta la aplicacion ya construida, sin dev server: Electron carga el export
 * estatico de Next a traves del esquema `app://`.
 *
 * Requiere haber ejecutado antes `npm run build`.
 */
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const desktopDir = join(root, 'apps', 'desktop')
const rendererOut = join(root, 'apps', 'renderer', 'out')

if (!existsSync(rendererOut)) {
  console.error('[start] falta apps/renderer/out. Ejecuta `npm run build` primero.')
  process.exit(1)
}

const electronPath = require('electron')
const child = spawn(electronPath, [desktopDir], {
  stdio: 'inherit',
  env: { ...process.env, SU_RENDERER_DIR: rendererOut },
})

child.on('exit', (code) => process.exit(code ?? 0))
