#!/usr/bin/env node
/**
 * Entorno de desarrollo: levanta el dev server de Next, espera a que responda,
 * compila el proceso principal y arranca Electron.
 *
 * Se escribe a mano en lugar de usar `concurrently` porque hace falta una cosa
 * que `concurrently` no da: no lanzar Electron hasta que Next este sirviendo.
 * Sin esa espera, la primera carga siempre falla con ERR_CONNECTION_REFUSED.
 *
 * ## Por que la ventana carga `localhost` y no `127.0.0.1`
 *
 * El cliente de desarrollo de Next abre su socket de recarga contra el mismo
 * origen desde el que se sirvio la pagina, y su servidor **rechaza esa
 * negociacion cuando el `Host` es una IP**: responde sin `101 Switching
 * Protocols`. Comprobado a mano contra este mismo servidor — con
 * `Host: 127.0.0.1:3456` no hay respuesta de upgrade y con `Host: localhost:3456`
 * contesta `101` y empieza a mandar mensajes.
 *
 * Y sin ese socket la pagina **no hidrata**: se ve entera (es el HTML que sirve
 * Next) y no responde absolutamente a nada —ni un clic, ni el estado del motor,
 * ni la carpeta de salida—, porque no hay un solo manejador montado. Es el peor
 * sintoma posible, porque parece que la aplicacion se ha colgado y no hay ningun
 * error escrito en ninguna parte. Con `localhost` hidrata.
 *
 * La comprobacion de salud sigue haciendose contra la IP: es la direccion que
 * existe siempre, tambien cuando el nombre no resuelve.
 */
import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const desktopDir = join(root, 'apps', 'desktop')
const port = Number(process.env['SU_PORT'] ?? 3456)
/** Direccion que se le pasa a la ventana. Ver la cabecera: no es una preferencia. */
const devUrl = `http://localhost:${port}`
/** Direccion que se sondea. Siempre existe, resuelva o no el nombre. */
const healthUrl = `http://127.0.0.1:${port}`

const isWindows = process.platform === 'win32'
const npmCmd = isWindows ? 'npm.cmd' : 'npm'
const children = []
let shuttingDown = false

function run(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: 'inherit',
    shell: isWindows,
    ...options,
  })
  children.push(child)
  return child
}

function shutdown(code = 0) {
  if (shuttingDown) return
  shuttingDown = true
  for (const child of children) {
    if (!child.killed) {
      try {
        child.kill()
      } catch {
        // El proceso ya habia terminado.
      }
    }
  }
  process.exit(code)
}

process.on('SIGINT', () => shutdown(0))
process.on('SIGTERM', () => shutdown(0))

async function waitForServer(url, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url, { method: 'GET' })
      if (response.status < 500) return true
    } catch {
      // Todavia no escucha.
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 300))
  }
  return false
}

console.log('[dev] compilando proceso principal…')
const buildResult = await new Promise((resolveBuild) => {
  const child = run(process.execPath, [join(root, 'scripts', 'build-main.mjs')])
  child.on('exit', (code) => resolveBuild(code ?? 1))
})
if (buildResult !== 0) {
  console.error('[dev] fallo la compilacion del proceso principal')
  shutdown(1)
}

console.log(`[dev] arrancando Next en ${devUrl} …`)
run(npmCmd, ['run', 'dev', '--workspace', '@superupscaly/renderer'])

const ready = await waitForServer(healthUrl)
if (!ready) {
  console.error(`[dev] Next no respondio en ${healthUrl} tras 60 s`)
  shutdown(1)
}

// El nombre tiene que responder tambien: es la direccion que cargara la ventana.
// Si aqui resolviera a una direccion en la que Next no escucha, la aplicacion
// quedaria pintada y sin hidratar, que es justo lo que este archivo evita.
let windowUrl = devUrl
if (!(await waitForServer(devUrl, 15_000))) {
  console.warn(
    `[dev] aviso: ${devUrl} no responde; se carga ${healthUrl}.\n` +
      '       El cliente de desarrollo puede no hidratar: si la interfaz no reacciona a\n' +
      '       los clics, es esto. Revisa /etc/hosts y que Next escuche en esa direccion.',
  )
  windowUrl = healthUrl
}

console.log(`[dev] lanzando Electron contra ${windowUrl}`)
const electronPath = require('electron')
const electron = run(electronPath, [desktopDir], {
  env: { ...process.env, SU_DEV_URL: windowUrl },
})

electron.on('exit', (code) => shutdown(code ?? 0))
