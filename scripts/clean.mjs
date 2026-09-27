#!/usr/bin/env node
/** Elimina los artefactos de build de todos los paquetes del monorepo. */
import { rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

const targets = [
  'apps/desktop/dist',
  'apps/renderer/.next',
  'apps/renderer/out',
  'apps/renderer/tsconfig.tsbuildinfo',
  'packages/shared/tsconfig.tsbuildinfo',
  'packages/ui/tsconfig.tsbuildinfo',
]

for (const target of targets) {
  rmSync(join(root, target), { recursive: true, force: true })
}

console.log('[clean] artefactos eliminados')
