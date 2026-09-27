import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  checkRatio,
  formatDuration,
  formatRatio,
  mean,
  median,
  percentile,
  ratio,
  standardDeviation,
  summarize,
} from './stats.mjs'

/**
 * Tests del harness de benchmark.
 *
 * Se ejecutan con `node --test scripts/lib` y no necesitan nada instalado: el
 * runner viene con Node. Las conclusiones del informe dependen de estas funciones,
 * asi que tienen que poder comprobarse por si mismas.
 */

test('el percentil usa rango mas cercano, no interpolacion', () => {
  // 20 muestras: el p95 por rango mas cercano es la 19ª, no un valor inventado
  // entre la 19ª y la 20ª.
  const values = Array.from({ length: 20 }, (_, index) => index + 1)

  assert.equal(percentile(values, 0.95), 19)
  assert.equal(percentile(values, 0.5), 10)
  assert.equal(percentile(values, 1), 20)
  assert.equal(percentile(values, 0.05), 1)
})

test('el percentil devuelve siempre un valor que existe en la muestra', () => {
  const values = [17, 3, 99, 42, 8, 61, 25]

  for (const p of [0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 1]) {
    assert.ok(
      values.includes(percentile(values, p)),
      `el p${p * 100} devolvio un valor que no esta en la muestra`,
    )
  }
})

test('el percentil no depende del orden de entrada', () => {
  const ascending = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
  const shuffled = [7, 2, 9, 1, 10, 4, 6, 3, 8, 5]

  assert.equal(percentile(shuffled, 0.9), percentile(ascending, 0.9))
})

test('el percentil rechaza valores fuera de rango', () => {
  assert.throws(() => percentile([1, 2, 3], 0), RangeError)
  assert.throws(() => percentile([1, 2, 3], 1.5), RangeError)
  assert.throws(() => percentile([1, 2, 3], -0.1), RangeError)
})

test('la mediana promedia el par central con un numero par de muestras', () => {
  assert.equal(median([3, 1, 2]), 2)
  assert.equal(median([4, 1, 3, 2]), 2.5)
  assert.equal(median([1]), 1)
})

test('la media y la desviacion tipica son las esperadas', () => {
  assert.equal(mean([1, 2, 3, 4]), 2.5)
  // Muestra [1,2,3]: media 2, varianza muestral (1+0+1)/2 = 1.
  assert.equal(standardDeviation([1, 2, 3]), 1)
})

test('la desviacion tipica de una sola muestra es cero, no NaN', () => {
  // Con una unica medicion no hay dispersion que calcular, pero devolver NaN
  // obligaria a tratar el caso en cada sitio que la use.
  assert.equal(standardDeviation([42]), 0)
})

test('las series vacias no producen NaN silenciosos', () => {
  const empty = summarize([])

  assert.equal(empty.count, 0)
  assert.ok(Number.isNaN(empty.median))
  assert.ok(Number.isNaN(empty.p95))
  // El consumidor comprueba `count` antes de usar los numeros.
})

test('el resumen de una serie coherente es correcto', () => {
  const summary = summarize([10, 20, 30, 40, 50])

  assert.equal(summary.count, 5)
  assert.equal(summary.min, 10)
  assert.equal(summary.max, 50)
  assert.equal(summary.mean, 30)
  assert.equal(summary.median, 30)
  assert.equal(summary.p95, 50)
})

test('el cociente compara en el sentido correcto', () => {
  // "candidate es 2x reference"
  assert.equal(ratio(10, 5), 2)
  assert.equal(ratio(5, 10), 0.5)
})

test('el cociente sin referencia valida es NaN, no cero', () => {
  // Devolver 0 o 1 seria inventarse un resultado: sin referencia no hay cociente.
  assert.ok(Number.isNaN(ratio(10, 0)))
  assert.ok(Number.isNaN(ratio(Number.NaN, 5)))
  assert.ok(Number.isNaN(ratio(10, Number.POSITIVE_INFINITY)))
})

test('el criterio de aceptacion devuelve veredicto y numeros', () => {
  const passing = checkRatio(10, 5, 2)
  assert.equal(passing.value, 2)
  assert.equal(passing.passed, true)
  assert.equal(passing.reason, null)

  const failing = checkRatio(9, 5, 2)
  assert.equal(failing.value, 1.8)
  assert.equal(failing.passed, false)
  // El veredicto va acompanado del numero que lo sustenta: un informe que solo
  // dice "cumple" obliga a confiar.
  assert.equal(failing.reason, null)
})

test('un criterio sin referencia no puede darse por cumplido', () => {
  const result = checkRatio(10, 0, 2)

  assert.equal(result.passed, false)
  assert.ok(result.reason !== null, 'deberia explicar por que no se puede evaluar')
})

test('las duraciones se formatean de forma legible', () => {
  assert.equal(formatDuration(500), '500 ms')
  assert.equal(formatDuration(1500), '1.50 s')
  assert.equal(formatDuration(90_000), '1 min 30 s')
  assert.equal(formatDuration(Number.NaN), '—')
})

test('los cocientes se formatean con dos decimales', () => {
  assert.equal(formatRatio(2.4), '2.40x')
  assert.equal(formatRatio(1), '1.00x')
  assert.equal(formatRatio(Number.NaN), '—')
})

test('la mediana aguanta un valor atipico que desplazaria la media', () => {
  // Es el caso real: una ejecucion lenta por el antivirus o porque otra
  // aplicacion estaba usando la GPU.
  const runs = [1000, 1010, 990, 1020, 45_000]

  assert.equal(median(runs), 1010)
  assert.ok(mean(runs) > 9000, 'la media si se desplaza')
})
