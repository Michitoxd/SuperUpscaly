#!/usr/bin/env node
/**
 * Genera los instaladores.
 *
 * Orden: compilar el sidecar, colocarlo donde electron-builder lo espera,
 * compilar el renderer y el proceso principal, y empaquetar.
 *
 * ## ONNX Runtime: una feature de compilacion y una biblioteca de ejecucion
 *
 * Son dos cosas distintas y confundirlas deja el instalador inservible:
 *
 * - La **feature** `onnx` decide si el binario sabe hablar con ONNX Runtime. Es
 *   de compilacion: sin ella, el sidecar empaquetado no puede inferir nunca, por
 *   muchos modelos que descargue el usuario. Se compila **con** ella.
 * - La **biblioteca** de ONNX Runtime no se enlaza: se carga en tiempo de
 *   ejecucion (ADR-018), asi que el instalador puede ser pequeno y los
 *   aceleradores de NVIDIA seguir llegando aparte como "Acceleration Packs"
 *   (ADR-012).
 *
 * Si la biblioteca no esta, el instalador sigue funcionando: el sidecar detecta
 * que no puede cargarla, lo dice en el registro y usa interpolacion clasica. Lo
 * que **no** puede pasar es empaquetar un binario sin la feature, porque entonces
 * ni siquiera existe la posibilidad de inferir.
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, copyFileSync, chmodSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const desktopDir = join(root, 'apps', 'desktop')
const inferenceDir = join(root, 'services', 'inference')

const platform = process.platform
const arch = process.arch
const isWindows = platform === 'win32'
const binaryName = isWindows ? 'su-cli.exe' : 'su-cli'

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    shell: isWindows,
    ...options,
  })
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} fallo con codigo ${result.status}`)
  }
}

function step(message) {
  console.log(`\n[package] ${message}`)
}

// --- 1. Sidecar -------------------------------------------------------------

step('compilando el sidecar (cargo build --release --features onnx -p su-cli)')

try {
  run('cargo', ['build', '--release', '--features', 'onnx', '-p', 'su-cli'], {
    cwd: inferenceDir,
  })
} catch (error) {
  console.error(
    '\n[package] No se pudo compilar el sidecar.\n' +
      'Comprueba que Rust esta instalado y que `cargo test` pasa:\n' +
      '  cd services/inference && cargo test\n',
  )
  throw error
}

const built = join(inferenceDir, 'target', 'release', binaryName)
if (!existsSync(built)) {
  throw new Error(`no se encontro el binario compilado en ${built}`)
}

// electron-builder espera los binarios separados por plataforma y arquitectura:
// un instalador de macOS puede ser universal.
const target = join(desktopDir, 'resources', 'bin', `${platform}-${arch}`)
mkdirSync(target, { recursive: true })

const destination = join(target, binaryName)
copyFileSync(built, destination)
if (!isWindows) {
  chmodSync(destination, 0o755)
}

console.log(`[package] sidecar -> ${destination}`)

// La biblioteca de ONNX Runtime viaja al lado del binario: es donde el sidecar la
// busca cuando el proceso principal no le pasa una ruta por `ORT_DYLIB_PATH`.
{
  const names = isWindows ? ['onnxruntime.dll'] : ['libonnxruntime.so', 'libonnxruntime.dylib']
  const directories = [join(root, 'runtime'), dirname(built)]

  let bundled = null

  for (const directory of directories) {
    if (!existsSync(directory)) continue

    const entries = readdirSync(directory)
    const chosen = entries
      .filter((name) => {
        const lower = name.toLowerCase()
        if (lower.includes('providers')) return false
        const isLibrary =
          lower.endsWith('.so') ||
          lower.endsWith('.so.1') ||
          lower.endsWith('.dylib') ||
          lower.endsWith('.dll') ||
          /\.so\.[\d.]+$/.test(lower)
        return isLibrary && names.some((prefix) => lower.startsWith(prefix.replace(/\.[^.]+$/, '')))
      })
      .sort((left, right) => left.length - right.length)[0]

    if (chosen) {
      bundled = join(directory, chosen)
      copyFileSync(bundled, join(target, chosen))
      console.log(`[package] ONNX Runtime -> ${join(target, chosen)}`)
      break
    }
  }

  if (!bundled) {
    console.warn(
      '[package] aviso: no se encontro la biblioteca de ONNX Runtime.\n' +
        '          El instalador funcionara, pero con interpolacion clasica hasta que se\n' +
        `          copie ${names[0]} en ${join(target, names[0])} o en ${join(root, 'runtime')}`,
    )
  }
}

// --- 2. Renderer ------------------------------------------------------------

step('compilando el renderer (next build)')
run('npm', ['run', 'build', '--workspace', '@superupscaly/renderer'], { cwd: root })

// --- 3. Proceso principal ---------------------------------------------------

step('compilando el proceso principal')
run(process.execPath, [join(root, 'scripts', 'build-main.mjs')], { cwd: root })

// --- 4. Instaladores --------------------------------------------------------

step('generando instaladores (electron-builder)')
run('npm', ['run', 'package', '--workspace', '@superupscaly/desktop'], { cwd: root })

step('listo. Los instaladores estan en release/')
