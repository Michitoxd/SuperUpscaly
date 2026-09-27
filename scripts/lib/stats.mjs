/**
 * Estadistica para el harness de benchmark.
 *
 * Se mantiene aparte y sin dependencias por una razon concreta: las conclusiones
 * del informe dependen de estos numeros, asi que tienen que poder verificarse por
 * si mismos. Un error aqui no rompe nada, simplemente produce un informe que dice
 * lo que no es.
 */

/**
 * Percentil por rango mas cercano.
 *
 * Se usa este metodo y no una interpolacion lineal porque lo que se mide son
 * latencias: el p95 debe ser **un tiempo que ocurrio de verdad**, no un valor
 * inventado entre dos mediciones. Para responder "que espera el usuario en el peor
 * caso" un tiempo real es mas honesto que una interpolacion.
 *
 * @param {number[]} values
 * @param {number} p entre 0 y 1
 */
export function percentile(values, p) {
  if (values.length === 0) return Number.NaN
  if (!(p > 0) || p > 1) {
    throw new RangeError(`percentil fuera de rango: ${p}`)
  }

  const sorted = [...values].sort((a, b) => a - b)
  const rank = Math.ceil(p * sorted.length)
  return sorted[Math.min(Math.max(rank, 1), sorted.length) - 1]
}

export function median(values) {
  if (values.length === 0) return Number.NaN

  const sorted = [...values].sort((a, b) => a - b)
  const middle = Math.floor(sorted.length / 2)

  // Con un numero par de muestras se promedia el par central: el percentil 50 por
  // rango mas cercano daria uno de los dos, que es mas fragil.
  return sorted.length % 2 === 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2
}

export function mean(values) {
  if (values.length === 0) return Number.NaN
  return values.reduce((total, value) => total + value, 0) / values.length
}

export function standardDeviation(values) {
  if (values.length < 2) return 0

  const average = mean(values)
  const variance =
    values.reduce((total, value) => total + (value - average) ** 2, 0) / (values.length - 1)

  return Math.sqrt(variance)
}

/**
 * Resumen de una serie de mediciones.
 *
 * @param {number[]} values
 */
export function summarize(values) {
  if (values.length === 0) {
    return {
      count: 0,
      min: Number.NaN,
      max: Number.NaN,
      mean: Number.NaN,
      median: Number.NaN,
      p95: Number.NaN,
      stdDev: Number.NaN,
    }
  }

  return {
    count: values.length,
    min: Math.min(...values),
    max: Math.max(...values),
    mean: mean(values),
    median: median(values),
    p95: percentile(values, 0.95),
    stdDev: standardDeviation(values),
  }
}

/**
 * Cociente entre dos mediciones, expresado como "cuantas veces a supera a b".
 *
 * Devuelve `NaN` si la referencia es cero o no es finita: un cociente sin
 * referencia no significa nada, y devolver 0 o 1 seria inventarse un resultado.
 */
export function ratio(candidate, reference) {
  if (!Number.isFinite(candidate) || !Number.isFinite(reference) || reference === 0) {
    return Number.NaN
  }
  return candidate / reference
}

/**
 * Comprueba un criterio de aceptacion del tipo "al menos N veces".
 *
 * Se devuelve el veredicto **y** los numeros que lo sustentan: un informe que solo
 * dice "cumple" obliga a confiar; uno que dice "2.4x, minimo exigido 2.0x" se
 * puede discutir.
 */
export function checkRatio(candidate, reference, minimum) {
  const value = ratio(candidate, reference)

  return {
    value,
    minimum,
    passed: Number.isFinite(value) && value >= minimum,
    reason: Number.isFinite(value)
      ? null
      : 'no hay referencia valida con la que comparar',
  }
}

/** Formatea una duracion en milisegundos de forma legible. */
export function formatDuration(ms) {
  if (!Number.isFinite(ms)) return '—'
  if (ms < 1000) return `${Math.round(ms)} ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)} s`

  const minutes = Math.floor(ms / 60_000)
  const seconds = Math.round((ms % 60_000) / 1000)
  return `${minutes} min ${String(seconds).padStart(2, '0')} s`
}

/** Formatea un cociente con dos decimales. */
export function formatRatio(value) {
  return Number.isFinite(value) ? `${value.toFixed(2)}x` : '—'
}

/**
 * Mediana de varias repeticiones de una misma medicion.
 *
 * Se usa la mediana y no la media porque una sola ejecucion lenta (el antivirus,
 * el compilador de TensorRT, otra aplicacion usando la GPU) desplazaria la media y
 * haria parecer lento un sistema que no lo es.
 */
export function medianOfRuns(runs) {
  return median(runs)
}
