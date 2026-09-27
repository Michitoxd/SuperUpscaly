#!/usr/bin/env node
/**
 * Harness de benchmark.
 *
 * Mide el rendimiento de SuperUpscaly sobre un conjunto de referencia fijo y, si
 * se le indica, lo compara con Upscayl sobre las mismas imagenes.
 *
 * ## Que mide y que no
 *
 * Mide **tiempo de pared de proceso completo**, que incluye el arranque. Es lo
 * mismo que se mide al otro lado, asi que la comparacion es justa; y es lo que el
 * usuario percibe cuando lanza un lote.
 *
 * **No mide calidad.** SSIM y LPIPS requieren decodificar las imagenes y comparar
 * pixeles, y eso vive en el sidecar (`su-cli compare`). Mezclar ambas cosas en el
 * mismo informe daria la falsa impresion de que un numero resume la herramienta.
 *
 * ## Por que la mediana y no la media
 *
 * Una sola ejecucion lenta (el antivirus, otra aplicacion usando la GPU, la
 * compilacion del motor de TensorRT en la primera pasada) desplaza la media y hace
 * parecer lento un sistema que no lo es. Se descarta ademas la primera repeticion,
 * que es la que paga la compilacion del motor.
 *
 * Uso:
 *   node scripts/benchmark.mjs --fixtures tests/fixtures --repeats 5
 *   node scripts/benchmark.mjs --fixtures tests/fixtures --upscayl-bin ~/upscayl-bin
 */

import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs'
import { dirname, extname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  checkRatio,
  formatDuration,
  formatRatio,
  median,
  summarize,
} from './lib/stats.mjs'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const isWindows = process.platform === 'win32'
const binaryName = isWindows ? 'su-cli.exe' : 'su-cli'

const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.webp', '.bmp', '.tif', '.tiff'])

/** Criterios de aceptacion del plan que este harness evalua. */
const TARGETS = {
  tensorrtVsNcnn: 2.0,
  cudaVsNcnn: 1.2,
}

// ---------------------------------------------------------------------------
// Argumentos
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const options = {
    fixtures: join(root, 'tests', 'fixtures'),
    output: join(root, 'docs', 'benchmarks'),
    repeats: 5,
    scale: 4,
    mode: 'photo',
    upscaylBin: process.env['UPSCAYL_BIN'] ?? null,
    upscaylModel: 'realesrgan-x4plus',
    sidecar: process.env['SU_SIDECAR_BIN'] ?? null,
    dryRun: false,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    const next = () => argv[++index]

    switch (arg) {
      case '--fixtures':
        options.fixtures = resolve(next())
        break
      case '--output':
        options.output = resolve(next())
        break
      case '--repeats':
        options.repeats = Number(next())
        break
      case '--scale':
        options.scale = Number(next())
        break
      case '--mode':
        options.mode = next()
        break
      case '--upscayl-bin':
        options.upscaylBin = next()
        break
      case '--upscayl-model':
        options.upscaylModel = next()
        break
      case '--sidecar':
        options.sidecar = next()
        break
      case '--dry-run':
        options.dryRun = true
        break
      case '--help':
      case '-h':
        printUsage()
        process.exit(0)
        break
      default:
        console.error(`Opcion desconocida: ${arg}`)
        printUsage()
        process.exit(2)
    }
  }

  if (!Number.isInteger(options.repeats) || options.repeats < 2) {
    console.error('--repeats necesita un entero >= 2 (la primera repeticion se descarta)')
    process.exit(2)
  }

  return options
}

function printUsage() {
  console.log(`Uso: node scripts/benchmark.mjs [opciones]

  --fixtures <dir>        Directorio con las imagenes de referencia
  --output <dir>          Donde escribir el informe (por defecto docs/benchmarks)
  --repeats <n>           Repeticiones por imagen (por defecto 5)
  --scale <2|4|8>         Factor de escala (por defecto 4)
  --mode <photo|illustration>
  --upscayl-bin <ruta>    Binario de Upscayl para comparar. Sin esto no hay comparacion
  --upscayl-model <id>    Modelo de Upscayl (por defecto realesrgan-x4plus)
  --sidecar <ruta>        Binario del sidecar. Por defecto se busca en target/release
  --dry-run               Muestra el plan sin ejecutar nada
`)
}

// ---------------------------------------------------------------------------
// Localizacion de binarios
// ---------------------------------------------------------------------------

function findSidecar(explicit) {
  const candidates = explicit
    ? [explicit]
    : [
        join(root, 'services', 'inference', 'target', 'release', binaryName),
        join(root, 'services', 'inference', 'target', 'debug', binaryName),
      ]

  return candidates.find((candidate) => existsSync(candidate)) ?? null
}

function listFixtures(directory) {
  if (!existsSync(directory)) return []

  return readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    // Los ocultos se ignoran. En macOS, cada archivo copiado a un volumen que no
    // sea APFS genera un `._nombre.png` que es un recurso fork, no una imagen: el
    // benchmark fallaria en todas las repeticiones de ese archivo fantasma.
    .filter((entry) => !entry.name.startsWith('.'))
    .filter((entry) => IMAGE_EXTENSIONS.has(extname(entry.name).toLowerCase()))
    .map((entry) => join(directory, entry.name))
    .sort()
}

// ---------------------------------------------------------------------------
// Medicion
// ---------------------------------------------------------------------------

/**
 * Ejecuta un comando midiendo su tiempo de pared.
 *
 * Se lanza con `stdio: 'pipe'` en lugar de heredar la salida: un benchmark que
 * imprime miles de lineas mientras mide distorsiona lo que mide.
 */
function timeCommand(command, args, timeoutMs = 10 * 60_000) {
  const started = process.hrtime.bigint()

  const result = spawnSync(command, args, {
    stdio: 'pipe',
    timeout: timeoutMs,
    windowsHide: true,
  })

  const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6

  return {
    elapsedMs,
    ok: result.status === 0,
    status: result.status,
    stderr: result.stderr?.toString().slice(0, 500) ?? '',
  }
}

/**
 * Mide una imagen varias veces y devuelve las muestras utiles.
 *
 * La primera repeticion se descarta: es la que paga el arranque en frio y, en
 * TensorRT, la compilacion del motor, que puede tardar minutos y no representa el
 * rendimiento en regimen estable.
 */
function measureImage(runner, image, options) {
  const samples = []
  const failures = []

  for (let repeat = 0; repeat < options.repeats; repeat += 1) {
    const result = runner(image, repeat)

    if (!result.ok) {
      failures.push(`repeticion ${repeat}: codigo ${result.status} — ${result.stderr}`)
      continue
    }

    // La primera se descarta, pero solo si hay mas de una util.
    if (repeat === 0) continue

    samples.push(result.elapsedMs)
  }

  return { samples, failures }
}

function runSuperUpscaly(sidecar, image, outputDir, options) {
  return timeCommand(sidecar, [
    'upscale',
    image,
    '--output',
    outputDir,
    '--scale',
    String(options.scale),
    '--mode',
    options.mode,
  ])
}

function runUpscayl(binary, image, outputDir, options) {
  return timeCommand(binary, [
    '-i',
    image,
    '-o',
    outputDir,
    '-n',
    options.upscaylModel,
    '-s',
    String(options.scale),
    '-f',
    'png',
  ])
}

// ---------------------------------------------------------------------------
// Informe
// ---------------------------------------------------------------------------

function buildReport({ options, sidecar, fixtures, ours, theirs }) {
  const timestamp = new Date().toISOString()

  const lines = [
    `# Informe de benchmark`,
    '',
    `Generado: ${timestamp}`,
    '',
    '## Entorno',
    '',
    '| | |',
    '|---|---|',
    `| Plataforma | ${process.platform} ${process.arch} |`,
    `| Node | ${process.version} |`,
    `| Sidecar | \`${sidecar}\` |`,
    `| Escala | x${options.scale} |`,
    `| Modo | ${options.mode} |`,
    `| Repeticiones por imagen | ${options.repeats} (se descarta la primera) |`,
    `| Imagenes | ${fixtures.length} |`,
    '',
    '## SuperUpscaly',
    '',
    '| Imagen | Mediana | p95 | Min | Max |',
    '|---|---|---|---|---|',
  ]

  for (const entry of ours.perImage) {
    if (entry.samples.length === 0) {
      lines.push(`| ${entry.name} | fallo | — | — | — |`)
      continue
    }

    const summary = summarize(entry.samples)
    lines.push(
      `| ${entry.name} | ${formatDuration(summary.median)} | ${formatDuration(summary.p95)} | ` +
        `${formatDuration(summary.min)} | ${formatDuration(summary.max)} |`,
    )
  }

  lines.push(
    '',
    `**Global**: mediana ${formatDuration(ours.medianMs)}, p95 ${formatDuration(ours.p95Ms)}`,
    '',
  )

  if (theirs) {
    lines.push(
      '## Upscayl',
      '',
      '| Imagen | Mediana | p95 |',
      '|---|---|---|',
    )

    for (const entry of theirs.perImage) {
      if (entry.samples.length === 0) {
        lines.push(`| ${entry.name} | fallo | — |`)
        continue
      }
      const summary = summarize(entry.samples)
      lines.push(`| ${entry.name} | ${formatDuration(summary.median)} | ${formatDuration(summary.p95)} |`)
    }

    lines.push(
      '',
      `**Global**: mediana ${formatDuration(theirs.medianMs)}, p95 ${formatDuration(theirs.p95Ms)}`,
      '',
      '## Comparacion',
      '',
      '| Criterio | Medido | Minimo exigido | Veredicto |',
      '|---|---|---|---|',
    )

    // El cociente se calcula sobre la mediana: es la medida robusta frente a
    // ejecuciones atipicas.
    const speedup = checkRatio(theirs.medianMs, ours.medianMs, TARGETS.tensorrtVsNcnn)
    lines.push(
      `| Aceleracion (mediana) | ${formatRatio(speedup.value)} | ` +
        `${formatRatio(TARGETS.tensorrtVsNcnn)} | ${speedup.passed ? 'Cumple' : 'No cumple'} |`,
    )

    const p95 = checkRatio(theirs.p95Ms, ours.p95Ms, 1.6)
    lines.push(
      `| Aceleracion (p95) | ${formatRatio(p95.value)} | ${formatRatio(1.6)} | ` +
        `${p95.passed ? 'Cumple' : 'No cumple'} |`,
    )

    lines.push(
      '',
      '> El objetivo de 2x corresponde a TensorRT (AC-10). Con CUDA sin TensorRT el',
      '> minimo exigido es 1.2x (AC-11). Comprueba el EP activo con',
      '> `su-cli capabilities` antes de interpretar estos numeros.',
      '',
    )
  } else {
    lines.push(
      '## Comparacion',
      '',
      '**No se pudo comparar con Upscayl**: no se indico `--upscayl-bin`.',
      '',
      'El objetivo del proyecto (AC-10) es duplicar el rendimiento de Upscayl con',
      'TensorRT. Sin referencia no se puede afirmar que se cumpla, asi que el informe',
      'no lo afirma.',
      '',
    )
  }

  lines.push(
    '## Fallos',
    '',
  )

  const allFailures = [
    ...ours.perImage.flatMap((entry) => entry.failures.map((failure) => `${entry.name}: ${failure}`)),
    ...(theirs?.perImage.flatMap((entry) => entry.failures.map((failure) => `${entry.name}: ${failure}`)) ?? []),
  ]

  if (allFailures.length === 0) {
    lines.push('Ninguno.', '')
  } else {
    for (const failure of allFailures) {
      lines.push(`- ${failure}`)
    }
    lines.push('')
  }

  lines.push(
    '## Notas metodologicas',
    '',
    '- Se mide **tiempo de pared de proceso completo**, incluido el arranque. Es lo',
    '  mismo al otro lado, asi que la comparacion es justa.',
    '- Se descarta la **primera repeticion** de cada imagen: paga el arranque en frio',
    '  y, con TensorRT, la compilacion del motor.',
    '- Se usa la **mediana** como medida principal. Una sola ejecucion lenta por el',
    '  antivirus o por otra aplicacion usando la GPU desplazaria la media.',
    '- Este informe **no mide calidad**. SSIM y LPIPS requieren comparar pixeles y se',
    '  obtienen con `su-cli compare` (ver docs/01-plan-de-proyecto.md, AC-20 a AC-22).',
    '',
  )

  return lines.join('\n')
}

// ---------------------------------------------------------------------------
// Principal
// ---------------------------------------------------------------------------

function main() {
  const options = parseArgs(process.argv.slice(2))

  const fixtures = listFixtures(options.fixtures)
  if (fixtures.length === 0) {
    console.error(
      `No hay imagenes en ${options.fixtures}.\n` +
        'Coloca ahi el conjunto de referencia (ver tests/fixtures/README.md).',
    )
    process.exit(1)
  }

  const sidecar = findSidecar(options.sidecar)
  const theirs = options.upscaylBin
    ? (existsSync(options.upscaylBin) ? options.upscaylBin : null)
    : null

  if (options.upscaylBin && !theirs) {
    console.warn(`Aviso: no existe ${options.upscaylBin}. Se omite la comparacion.`)
  }

  console.log(`Sidecar   ${sidecar ?? 'NO ENCONTRADO'}`)
  console.log(`Imagenes  ${fixtures.length} en ${options.fixtures}`)
  console.log(`Repeticiones ${options.repeats} (se descarta la primera)`)
  console.log(`Upscayl   ${theirs ?? 'no indicado, sin comparacion'}`)
  console.log('')

  // El modo de prueba sirve justamente para comprobar el plan antes de tener
  // todo compilado, asi que no puede exigir el binario.
  if (options.dryRun) {
    for (const fixture of fixtures) {
      console.log(`  mediria ${fixture}`)
    }
    return
  }

  if (!sidecar) {
    console.error(
      'No se encontro el sidecar.\n' +
        'Compilalo con: cd services/inference && cargo build --release -p su-cli\n' +
        'O indica su ruta con --sidecar o la variable SU_SIDECAR_BIN.',
    )
    process.exit(1)
  }

  const workDir = join(options.output, '.work')
  mkdirSync(workDir, { recursive: true })

  const ours = { perImage: [], medianMs: Number.NaN, p95Ms: Number.NaN }
  const theirResults = theirs ? { perImage: [], medianMs: Number.NaN, p95Ms: Number.NaN } : null

  for (const fixture of fixtures) {
    const name = fixture.split(/[\\/]/).pop()
    process.stdout.write(`  ${name} … `)

    const ourResult = measureImage(
      (image) => runSuperUpscaly(sidecar, image, join(workDir, 'su'), options),
      fixture,
      options,
    )
    ours.perImage.push({ name, ...ourResult })
    process.stdout.write(`${formatDuration(median(ourResult.samples))}`)

    if (theirs && theirResults) {
      const theirResult = measureImage(
        (image) => runUpscayl(theirs, image, join(workDir, 'upscayl'), options),
        fixture,
        options,
      )
      theirResults.perImage.push({ name, ...theirResult })
      process.stdout.write(` vs ${formatDuration(median(theirResult.samples))}`)
    }

    console.log('')
  }

  // Global: se combinan todas las muestras. La mediana del conjunto no es la media
  // de las medianas, que ponderaria igual una imagen pequena y una grande.
  const allOurs = ours.perImage.flatMap((entry) => entry.samples)
  ours.medianMs = median(allOurs)
  ours.p95Ms = summarize(allOurs).p95

  if (theirResults) {
    const allTheirs = theirResults.perImage.flatMap((entry) => entry.samples)
    theirResults.medianMs = median(allTheirs)
    theirResults.p95Ms = summarize(allTheirs).p95
  }

  mkdirSync(options.output, { recursive: true })
  const stamp = new Date().toISOString().slice(0, 10)
  const reportPath = join(options.output, `${stamp}.md`)

  writeFileSync(
    reportPath,
    buildReport({ options, sidecar, fixtures, ours, theirs: theirResults }),
    'utf8',
  )

  console.log('')
  console.log(`Informe: ${reportPath}`)
}

main()
