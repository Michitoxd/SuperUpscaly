/**
 * Imagenes locales hacia el renderer.
 *
 * ## Por que hace falta un contrato
 *
 * La ventana no carga desde `file://` sino desde `app://superupscaly`, y su CSP
 * declara `img-src 'self'`: el renderer **no puede** leer una imagen del disco
 * por su ruta, ni con `file://` (que ademas dejaria `'self'` sin significado).
 *
 * La salida es servirla por el mismo esquema que ya cubre `'self'`:
 *
 *     app://superupscaly/media?p=<ruta absoluta codificada>
 *
 * Este modulo fija ese formato y lo resuelve. Vive en `shared` y no en el
 * proceso principal porque lo construye la interfaz y lo atiende el proceso
 * principal: si cada lado tuviera su propia version del formato, una discrepancia
 * —un parametro renombrado, una barra de mas— dejaria la comparacion antes y
 * despues en blanco sin decir por que.
 *
 * ## Lo que esto NO es
 *
 * No es un lector de archivos. El proceso principal solo sirve rutas que el
 * mismo registro antes: imagenes que ya pasaron la validacion o resultados que
 * acaba de escribir el motor. Una ruta que el renderer se invente se rechaza con
 * un 403 y queda en el registro de la aplicacion (ver `main/media/registry.ts`).
 */

/** Esquema propio de la aplicacion; se registra como privilegiado al arrancar. */
export const APP_SCHEME = 'app'

/** Origen real del renderer. `'self'` en la CSP es exactamente este origen. */
export const APP_ORIGIN = `${APP_SCHEME}://superupscaly`

/** Ruta, dentro del esquema de la aplicacion, bajo la que se sirven las imagenes. */
export const MEDIA_ROUTE = '/media'

/** Parametro de consulta por el que viaja la ruta absoluta. */
export const MEDIA_PARAM = 'p'

/** URL con la que el renderer pide una imagen local. */
export function mediaUrl(absolutePath: string): string {
  return `${APP_ORIGIN}${MEDIA_ROUTE}?${MEDIA_PARAM}=${encodeURIComponent(absolutePath)}`
}

/**
 * Ruta absoluta que pide una URL de medios, o `null` si la URL no es de medios
 * o no trae ruta.
 *
 * Solo comprueba la forma de la URL; que la ruta este autorizada lo decide el
 * proceso principal, que es quien tiene el registro.
 */
export function parseMediaUrl(rawUrl: string): string | null {
  let url: URL
  try {
    url = new URL(rawUrl)
  } catch {
    return null
  }

  if (url.protocol !== `${APP_SCHEME}:`) return null
  if (url.host !== 'superupscaly') return null
  if (url.pathname !== MEDIA_ROUTE) return null

  const requested = url.searchParams.get(MEDIA_PARAM)
  return requested === null || requested.length === 0 ? null : requested
}

/** true si la ruta de una peticion corresponde al servicio de imagenes. */
export function isMediaPath(pathname: string): boolean {
  return pathname === MEDIA_ROUTE
}
