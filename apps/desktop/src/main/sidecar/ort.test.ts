/**
 * Pruebas de la localizacion de ONNX Runtime.
 *
 * El fallo que estas pruebas evitan es silencioso y caro: si el sidecar no recibe
 * `ORT_DYLIB_PATH`, `ort` intenta cargar `libonnxruntime.so` por nombre —que en
 * Linux es la que traiga el sistema, normalmente ninguna o una con otra version—
 * y el motor cae al clasico. El usuario ve resultados peores sin saber por que, y
 * el registro no dice nada.
 *
 * Se comprueban las funciones puras (orden de preferencia y eleccion del nombre),
 * que es donde estan las decisiones. El acceso al disco se prueba aparte y solo
 * para lo que no se puede decidir sin el.
 */

import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

import {
  ortLibraryCandidates,
  pickRuntimeLibrary,
  resolveOrtLibrary,
  runtimeLibraryNames,
} from './ort.ts'

test('el nombre canonico de la biblioteca depende de la plataforma', () => {
  assert.deepEqual(runtimeLibraryNames('linux'), ['libonnxruntime.so'])
  assert.deepEqual(runtimeLibraryNames('darwin'), ['libonnxruntime.dylib'])
  assert.deepEqual(runtimeLibraryNames('win32'), ['onnxruntime.dll'])
})

test('lo que el usuario fije en ORT_DYLIB_PATH manda', () => {
  const candidates = ortLibraryCandidates({
    sidecarPath: '/app/bin/su-cli',
    dataDir: '/datos',
    platform: 'linux',
    existing: '/opt/ort/libonnxruntime.so',
  })

  assert.deepEqual(candidates, ['/opt/ort/libonnxruntime.so'])
})

test('una variable vacia no impide buscar en los sitios de siempre', () => {
  const candidates = ortLibraryCandidates({
    sidecarPath: '/app/bin/su-cli',
    dataDir: '/datos',
    platform: 'linux',
    existing: '   ',
  })

  // El runtime instalado por la aplicacion va antes que el que esta junto al
  // binario: sobrevive a recompilar el sidecar.
  assert.deepEqual(candidates, [
    '/datos/runtime/libonnxruntime.so',
    '/app/bin/libonnxruntime.so',
  ])
})

test('la biblioteca versionada vale cuando no hay nombre canonico', () => {
  const chosen = pickRuntimeLibrary(
    ['libonnxruntime.so.1.28.2', 'libonnxruntime.so.1', 'libonnxruntime.so'],
    'linux',
  )

  assert.equal(chosen, 'libonnxruntime.so')
})

test('sin nombre canonico se elige la version mas corta', () => {
  const chosen = pickRuntimeLibrary(['libonnxruntime.so.1.28.2', 'libonnxruntime.so.1'], 'linux')

  assert.equal(chosen, 'libonnxruntime.so.1')
})

test('una biblioteca de execution providers no es el motor', () => {
  const chosen = pickRuntimeLibrary(
    ['libonnxruntime_providers_cuda.so', 'libonnxruntime_providers_shared.so'],
    'linux',
  )

  assert.equal(chosen, null)
})

test('un archivo que no es una biblioteca se ignora', () => {
  assert.equal(pickRuntimeLibrary(['onnxruntime-gpu.txt', 'notas.md'], 'linux'), null)
})

test('en windows se reconoce onnxruntime.dll', () => {
  assert.equal(pickRuntimeLibrary(['onnxruntime.dll'], 'win32'), 'onnxruntime.dll')
})

test('se encuentra el runtime junto al sidecar aunque este versionado', () => {
  const base = mkdtempSync(join(tmpdir(), 'su-ort-'))
  const bin = join(base, 'bin')
  const data = join(base, 'datos')
  mkdirSync(bin)
  mkdirSync(data)

  writeFileSync(join(bin, 'libonnxruntime.so.1.28.2'), 'no es una biblioteca de verdad')

  const resolved = resolveOrtLibrary({
    sidecarPath: join(bin, 'su-cli'),
    dataDir: data,
    platform: 'linux',
  })

  assert.equal(resolved, join(bin, 'libonnxruntime.so.1.28.2'))
})

test('el runtime de la aplicacion tiene preferencia sobre el del binario', () => {
  const base = mkdtempSync(join(tmpdir(), 'su-ort-'))
  const bin = join(base, 'bin')
  const runtime = join(base, 'datos', 'runtime')
  mkdirSync(bin, { recursive: true })
  mkdirSync(runtime, { recursive: true })

  writeFileSync(join(bin, 'libonnxruntime.so'), 'junto al binario')
  writeFileSync(join(runtime, 'libonnxruntime.so'), 'instalado por la aplicacion')

  const resolved = resolveOrtLibrary({
    sidecarPath: join(bin, 'su-cli'),
    dataDir: join(base, 'datos'),
    platform: 'linux',
  })

  assert.equal(resolved, join(runtime, 'libonnxruntime.so'))
})

test('sin runtime en ningun sitio se devuelve null, no una ruta inventada', () => {
  const base = mkdtempSync(join(tmpdir(), 'su-ort-'))

  const resolved = resolveOrtLibrary({
    sidecarPath: join(base, 'su-cli'),
    dataDir: join(base, 'datos'),
    platform: 'linux',
  })

  assert.equal(resolved, null)
})

test('una ruta explicita que no existe no se sustituye por otra', () => {
  const base = mkdtempSync(join(tmpdir(), 'su-ort-'))
  const bin = join(base, 'bin')
  mkdirSync(bin, { recursive: true })
  writeFileSync(join(bin, 'libonnxruntime.so'), 'junto al binario')

  const resolved = resolveOrtLibrary({
    sidecarPath: join(bin, 'su-cli'),
    dataDir: join(base, 'datos'),
    platform: 'linux',
    existing: '/opt/ort/que-no-existe.so',
  })

  assert.equal(resolved, null)
})
