import { app, BrowserWindow, screen, shell } from 'electron'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { APP_ORIGIN } from '@superupscaly/shared'
import { logger } from './logging/logger'

interface WindowState {
  width: number
  height: number
  x: number | null
  y: number | null
  maximized: boolean
}

const DEFAULT_SIZE = { width: 1280, height: 860 }
const MIN_SIZE = { width: 1024, height: 700 }

function statePath(): string {
  return join(app.getPath('userData'), 'window-state.json')
}

function loadWindowState(): WindowState {
  const fallback: WindowState = { ...DEFAULT_SIZE, x: null, y: null, maximized: false }
  try {
    const parsed: unknown = JSON.parse(readFileSync(statePath(), 'utf8'))
    if (typeof parsed !== 'object' || parsed === null) return fallback
    const candidate = parsed as Partial<WindowState>
    const state: WindowState = {
      width: typeof candidate.width === 'number' ? candidate.width : DEFAULT_SIZE.width,
      height: typeof candidate.height === 'number' ? candidate.height : DEFAULT_SIZE.height,
      x: typeof candidate.x === 'number' ? candidate.x : null,
      y: typeof candidate.y === 'number' ? candidate.y : null,
      maximized: candidate.maximized === true,
    }
    // Si la pantalla cambio de tamano (o se desconecto un monitor), la posicion
    // guardada puede quedar fuera del escritorio visible.
    if (state.x !== null && state.y !== null) {
      const visible = screen.getAllDisplays().some((display) => {
        const { x, y, width, height } = display.workArea
        return (
          state.x !== null &&
          state.y !== null &&
          state.x < x + width &&
          state.y < y + height &&
          state.x + state.width > x &&
          state.y + state.height > y
        )
      })
      if (!visible) {
        state.x = null
        state.y = null
      }
    }
    return state
  } catch {
    return fallback
  }
}

function saveWindowState(win: BrowserWindow): void {
  if (win.isDestroyed()) return
  try {
    const bounds = win.getNormalBounds()
    const state: WindowState = {
      width: bounds.width,
      height: bounds.height,
      x: bounds.x,
      y: bounds.y,
      maximized: win.isMaximized(),
    }
    mkdirSync(app.getPath('userData'), { recursive: true })
    writeFileSync(statePath(), `${JSON.stringify(state, null, 2)}\n`, 'utf8')
  } catch (error) {
    logger.warn('window.state-save-failed', { code: (error as NodeJS.ErrnoException).code ?? 'unknown' })
  }
}

export interface CreateWindowOptions {
  /** URL del dev server de Next. Si falta, se carga desde el esquema `app://`. */
  devUrl?: string | undefined
  /** Directorio del export estatico de Next (solo produccion). */
  rendererDir?: string | undefined
}

export function createMainWindow(options: CreateWindowOptions): BrowserWindow {
  const state = loadWindowState()

  const win = new BrowserWindow({
    width: state.width,
    height: state.height,
    ...(state.x !== null && state.y !== null ? { x: state.x, y: state.y } : {}),
    minWidth: MIN_SIZE.width,
    minHeight: MIN_SIZE.height,
    show: false,
    backgroundColor: '#1E1B2E',
    autoHideMenuBar: true,
    title: 'SuperUpscaly',
    ...(process.platform === 'darwin' ? { titleBarStyle: 'hiddenInset' as const } : {}),
    webPreferences: {
      preload: join(__dirname, '..', 'preload', 'index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      spellcheck: false,
      devTools: !app.isPackaged,
    },
  })

  if (state.maximized) win.maximize()

  // --- Seguridad de navegacion -------------------------------------------
  // La ventana es una aplicacion, no un navegador: nada de ventanas emergentes
  // ni de navegar fuera del propio renderer.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith('https://')) void shell.openExternal(url)
    return { action: 'deny' }
  })

  win.webContents.on('will-navigate', (event, url) => {
    const isDevServer = options.devUrl !== undefined && url.startsWith(options.devUrl)
    if (!url.startsWith(APP_ORIGIN) && !isDevServer) {
      event.preventDefault()
      logger.warn('window.navigation-blocked', { url })
    }
  })

  // --- Recuperacion ante fallos ------------------------------------------
  win.webContents.on('render-process-gone', (_event, details) => {
    logger.error('window.render-process-gone', { reason: details.reason, exitCode: details.exitCode })
    if (details.reason !== 'clean-exit' && !win.isDestroyed()) {
      win.reload()
    }
  })

  win.on('unresponsive', () => logger.warn('window.unresponsive'))
  win.on('responsive', () => logger.info('window.responsive'))

  // --- Diagnostico del renderer -------------------------------------------
  // Una ventana que se ve pero no responde es el peor fallo posible, y hasta
  // ahora no dejaba ni una linea en el registro: un script bloqueado por la CSP
  // o un fallo de hidratacion solo se veian abriendo las herramientas de
  // desarrollo, que en una aplicacion empaquetada no existen. Estos tres eventos
  // convierten "no funciona" en un motivo escrito.
  win.webContents.on('console-message', (details) => {
    const context = {
      message: details.message.slice(0, 600),
      source: details.sourceId,
      line: details.lineNumber,
    }
    if (details.level === 'error') {
      logger.error('renderer.console-error', context)
    } else if (details.level === 'warning') {
      logger.warn('renderer.console-warning', context)
    }
  })

  win.webContents.on('preload-error', (_event, preloadPath, error) => {
    // Sin preload no hay `window.su`, asi que la interfaz entera se queda sin
    // forma de hablar con el motor y los botones parecen no hacer nada.
    logger.error('renderer.preload-error', { preloadPath, message: error.message })
  })

  win.webContents.on('did-fail-load', (_event, errorCode, errorDescription, url, isMainFrame) => {
    if (isMainFrame) {
      logger.error('renderer.load-failed', { errorCode, errorDescription, url })
    }
  })

  win.once('ready-to-show', () => {
    win.show()
    logger.info('window.shown')
  })

  win.on('close', () => saveWindowState(win))
  win.on('closed', () => logger.info('window.closed'))

  if (options.devUrl) {
    void win.loadURL(options.devUrl)
    win.webContents.openDevTools({ mode: 'detach' })
    logger.info('window.load-dev-url', { url: options.devUrl })
  } else if (options.rendererDir) {
    void win.loadURL(`${APP_ORIGIN}/index.html`)
    logger.info('window.load-app-scheme')
  } else {
    logger.error('window.no-renderer-source')
    void win.loadURL(
      'data:text/html,<body style="font-family:sans-serif;background:%231E1B2E;color:%23F3F4F6;padding:40px">' +
        '<h1>No se encontro el renderer</h1><p>Ejecuta <code>npm run build</code> antes de iniciar.</p></body>',
    )
  }

  return win
}
