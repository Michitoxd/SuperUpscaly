import { app, BrowserWindow, Menu } from 'electron'
import { join } from 'node:path'
import { registerIpcHandlers } from './ipc'
import { registerSidecarIpc } from './ipc/sidecar'
import { initLogger, logger } from './logging/logger'
import { registerAppProtocolHandler, registerAppScheme } from './protocol'
import { SidecarClient } from './sidecar/client'
import { SidecarSupervisor } from './sidecar/supervisor'
import { loadSettings } from './store/settings'
import { createMainWindow } from './windows'

/**
 * El nombre debe fijarse antes de que se resuelva `userData`, porque de el
 * dependen la carpeta de ajustes, la de logs y la de cache de modelos.
 */
app.setName('SuperUpscaly')

// Debe registrarse antes de `whenReady`.
registerAppScheme()

const hasSingleInstanceLock = app.requestSingleInstanceLock()

if (!hasSingleInstanceLock) {
  // Otra instancia ya esta en marcha: nos retiramos sin ruido.
  app.quit()
} else {
  let mainWindow: BrowserWindow | null = null

  app.on('second-instance', () => {
    if (!mainWindow || mainWindow.isDestroyed()) return
    if (mainWindow.isMinimized()) mainWindow.restore()
    mainWindow.focus()
    logger.info('app.second-instance-focused')
  })

  app.on('web-contents-created', (_event, contents) => {
    // Ningun webContents puede crear ventanas ni navegar por su cuenta.
    contents.on('will-attach-webview', (event) => event.preventDefault())
  })

  void app.whenReady().then(() => {
    initLogger()
    logger.info('app.starting', {
      version: app.getVersion(),
      electron: process.versions.electron ?? 'unknown',
      platform: process.platform,
      arch: process.arch,
      packaged: app.isPackaged,
    })

    // Los ajustes se cargan al arrancar para detectar un JSON corrupto cuanto
    // antes y dejar constancia en el log.
    loadSettings()

    registerIpcHandlers()

    // El sidecar se arranca en segundo plano: la ventana debe aparecer de
    // inmediato y mostrar "arrancando", no quedarse en negro esperando a un
    // proceso que puede tardar.
    const supervisor = new SidecarSupervisor({
      isPackaged: app.isPackaged,
      resourcesPath: process.resourcesPath,
      appPath: app.getAppPath(),
      dataDir: app.getPath('userData'),
      override: process.env['SU_SIDECAR_BIN'],
    })
    const client = new SidecarClient(supervisor)
    const modelDownloads = registerSidecarIpc(supervisor, client)
    void supervisor.start()

    // Cierre: se da al sidecar un margen acotado para apagarse solo.
    //
    // `stop()` pide el cierre ordenado por HTTP y, si no hay respuesta, envia
    // senales: es lo que marca el trabajo en curso como interrumpido, que es lo
    // que permite reanudarlo despues. Lanzarlo sin esperarlo (`void`) dejaba al
    // proceso muriendo por su cuenta mientras el sidecar seguia trabajando, asi
    // que el margen existe; pero esperarlo sin limite convertiria un sidecar
    // atascado en una aplicacion que no se cierra, y eso es peor. De ahi el
    // limite: el cierre ordenado suele contestar en milisegundos.
    let sidecarReleased = false
    const releaseDeadlineMs = 2_500

    app.on('before-quit', (event) => {
      // Una descarga a medias deja un `.part` que sirve para reanudar, asi que
      // cortarla al cerrar no pierde trabajo: lo que no puede quedar es una
      // escritura en vuelo sobre un proceso que ya se esta muriendo.
      modelDownloads.cancelAll()

      if (sidecarReleased) return

      event.preventDefault()

      const deadline = new Promise<void>((resolve) => {
        setTimeout(resolve, releaseDeadlineMs)
      })

      void Promise.race([supervisor.stop(), deadline]).finally(() => {
        sidecarReleased = true
        app.quit()
      })
    })

    const devUrl = process.env['SU_DEV_URL']
    const rendererDir = process.env['SU_RENDERER_DIR'] ?? (app.isPackaged ? join(process.resourcesPath, 'renderer') : undefined)

    if (rendererDir) {
      registerAppProtocolHandler(rendererDir)
    }

    mainWindow = createMainWindow({
      devUrl: devUrl && devUrl.length > 0 ? devUrl : undefined,
      rendererDir: devUrl ? undefined : rendererDir,
    })

    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) {
        mainWindow = createMainWindow({
          devUrl: devUrl && devUrl.length > 0 ? devUrl : undefined,
          rendererDir: devUrl ? undefined : rendererDir,
        })
      }
    })

    applyMenu()
  })

  app.on('window-all-closed', () => {
    // En macOS es normal que una app siga viva sin ventanas; en el resto de
    // plataformas, cerrar la ultima ventana significa salir.
    if (process.platform !== 'darwin') app.quit()
  })

  app.on('before-quit', () => {
    logger.info('app.quitting')
  })

  process.on('uncaughtException', (error) => {
    logger.error('app.uncaught-exception', { message: error.message, stack: error.stack ?? '' })
  })

  process.on('unhandledRejection', (reason) => {
    logger.error('app.unhandled-rejection', { reason: String(reason) })
  })
}

/**
 * En Windows y Linux el menu se oculta por completo: la interfaz es la app.
 * En macOS es obligatorio mantener uno minimo para que funcionen los atajos
 * de copiar/pegar/cerrar y el menu de aplicacion.
 */
function applyMenu(): void {
  if (process.platform !== 'darwin') {
    Menu.setApplicationMenu(null)
    return
  }

  const menu = Menu.buildFromTemplate([
    {
      label: app.getName(),
      submenu: [
        { role: 'about' },
        { type: 'separator' },
        { role: 'hide' },
        { role: 'hideOthers' },
        { role: 'unhide' },
        { type: 'separator' },
        { role: 'quit' },
      ],
    },
    {
      label: 'Editar',
      submenu: [
        { role: 'undo' },
        { role: 'redo' },
        { type: 'separator' },
        { role: 'cut' },
        { role: 'copy' },
        { role: 'paste' },
        { role: 'selectAll' },
      ],
    },
    {
      label: 'Ventana',
      submenu: [{ role: 'minimize' }, { role: 'zoom' }, { role: 'togglefullscreen' }],
    },
  ])

  Menu.setApplicationMenu(menu)
}
