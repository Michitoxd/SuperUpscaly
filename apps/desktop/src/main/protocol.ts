import { net, protocol } from 'electron'
import { basename, join, normalize, sep } from 'node:path'
import { pathToFileURL } from 'node:url'
import { APP_ORIGIN, APP_SCHEME, isMediaPath, parseMediaUrl } from '@superupscaly/shared'
import { contentSecurityPolicy } from './csp'
import { logger } from './logging/logger'
import { isMediaAllowed } from './media/registry'

/**
 * En produccion el renderer no se carga desde `file://` sino desde un esquema
 * propio (`app://superupscaly/...`).
 *
 * Motivo: con `file://` las rutas absolutas que genera Next (`/_next/static/...`)
 * apuntan a la raiz del disco. Un esquema registrado como `standard` + `secure`
 * da un origen real, de modo que `'self'` en la CSP significa algo y el renderer
 * no necesita privilegios de `file://`.
 */
/** Debe llamarse ANTES de `app.whenReady()`. */
export function registerAppScheme(): void {
  protocol.registerSchemesAsPrivileged([
    {
      scheme: APP_SCHEME,
      privileges: {
        standard: true,
        secure: true,
        supportFetchAPI: true,
        stream: true,
        corsEnabled: false,
      },
    },
  ])
}

/** Debe llamarse DESPUES de `app.whenReady()`. */
export function registerAppProtocolHandler(rendererDir: string): void {
  const root = normalize(rendererDir)

  protocol.handle(APP_SCHEME, async (request) => {
    let pathname: string
    try {
      pathname = decodeURIComponent(new URL(request.url).pathname)
    } catch {
      return new Response('Bad request', { status: 400 })
    }

    // Las imagenes que la interfaz muestra (la original y el resultado, para la
    // comparacion antes y despues) se sirven por este mismo esquema, no por
    // `file://`: asi `img-src 'self'` de la CSP las cubre sin abrir nada mas, y la
    // ruta no puede escapar del directorio del renderer porque no es un archivo del
    // renderer — viene en la consulta y solo se sirve si el proceso principal la
    // registro antes (ver `media/registry.ts`).
    if (isMediaPath(pathname)) {
      const requested = parseMediaUrl(request.url)
      if (requested === null || !isMediaAllowed(requested)) {
        logger.warn('protocol.media-denied', {
          name: requested === null ? '' : basename(requested),
        })
        return new Response('Forbidden', { status: 403 })
      }
      return await serveMedia(requested)
    }

    if (pathname === '' || pathname === '/') pathname = '/index.html'

    const target = normalize(join(root, pathname))
    // Sin esta comprobacion, una peticion a `app://superupscaly/../../etc/passwd`
    // saldria del directorio del renderer.
    if (target !== root && !target.startsWith(root + sep)) {
      logger.warn('protocol.path-escape-blocked')
      return new Response('Forbidden', { status: 403 })
    }

    try {
      const response = await net.fetch(pathToFileURL(target).toString())
      if (!target.toLowerCase().endsWith('.html')) {
        return response
      }

      // El cuerpo se lee entero, en lugar de reenviar la corriente, porque la CSP
      // lleva el hash de los scripts en linea que contiene **este** HTML (ver
      // `csp.ts`). Son decenas de KB: el coste es despreciable y a cambio la
      // cabecera no puede desincronizarse del documento que acompana.
      const html = await response.text()
      const headers = new Headers(response.headers)
      headers.set('Content-Security-Policy', contentSecurityPolicy(html))
      return new Response(html, { status: response.status, headers })
    } catch (error) {
      // Un HTML que no se sirve deja la ventana en blanco: el motivo tiene que
      // quedar escrito, porque desde fuera solo se ve una aplicacion vacia.
      logger.error('protocol.serve-failed', {
        target,
        message: error instanceof Error ? error.message : String(error),
      })
      return new Response('Not found', { status: 404 })
    }
  })

  logger.info('protocol.registered', { root })
}

/**
 * Devuelve una imagen del disco para la ventana.
 *
 * `no-store` no es un detalle: un escalado repetido sobre el mismo archivo de
 * salida usa exactamente la misma URL, y con la cache puesta la comparacion
 * seguiria enseñando el resultado anterior —una imagen que se ve, sin error y sin
 * ser la que se acaba de escribir—. El coste de releerla es despreciable al lado de
 * eso; cada imagen se carga una vez, cuando el usuario pide compararla.
 */
async function serveMedia(absolutePath: string): Promise<Response> {
  try {
    const response = await net.fetch(pathToFileURL(absolutePath).toString())
    if (!response.ok) {
      logger.warn('protocol.media-missing', { name: basename(absolutePath), status: response.status })
      return new Response('Not found', { status: 404 })
    }

    const headers = new Headers(response.headers)
    headers.set('Cache-Control', 'no-store')
    return new Response(response.body, { status: response.status, headers })
  } catch (error) {
    // Un archivo que desaparece entre el registro y la peticion deja la imagen en
    // blanco: el motivo queda escrito, porque desde fuera solo se ve un hueco.
    logger.error('protocol.media-failed', {
      name: basename(absolutePath),
      message: error instanceof Error ? error.message : String(error),
    })
    return new Response('Not found', { status: 404 })
  }
}
