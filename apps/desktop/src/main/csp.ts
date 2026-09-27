import { createHash } from 'node:crypto'

/**
 * Politica de seguridad de contenido del renderer.
 *
 * ## Por que los hashes
 *
 * El export estatico de Next no arranca solo con scripts externos: incluye dos
 * `<script>` **sin `src`**, el que crea `self.__next_f` y el que empuja los datos
 * del arbol de React. Con un `script-src 'self'` a secas, Chromium los bloquea.
 *
 * El resultado es el peor fallo posible y por eso tiene su propio modulo: la
 * pagina **se ve** —el HTML y el CSS son estaticos y se cargan— pero no se
 * hidrata. Ningun boton responde, no se abre ningun dialogo y no aparece ningun
 * error a la vista. Ademas solo ocurre en la aplicacion construida: en desarrollo
 * el renderer se sirve por HTTP, donde no se aplica esta cabecera.
 *
 * Se calculan hashes en lugar de permitir `'unsafe-inline'` porque la CSP existe
 * justamente para que `script-src 'self'` signifique algo; anadir `'unsafe-inline'`
 * la dejaria sin efecto sobre scripts y convertiria una defensa real en un
 * parrafo decorativo.
 *
 * ## Lo que hace que no puedan dejar de coincidir
 *
 * Los hashes se calculan sobre **el mismo texto que se sirve**, no sobre una
 * lectura aparte del archivo. Un hash de un archivo que luego se sirve distinto
 * —por un salto de linea normalizado, por una transformacion— deja la aplicacion
 * muda otra vez, asi que la unica fuente posible es el cuerpo que va a viajar.
 */

/** Directivas que no dependen del contenido. */
const BASE_DIRECTIVES = [
  "default-src 'self'",
  "img-src 'self' data: blob:",
  "style-src 'self' 'unsafe-inline'",
  "font-src 'self' data:",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
]

/**
 * Scripts en linea de un HTML.
 *
 * El patron exige que la etiqueta no tenga `src`: un script externo lo cubre
 * `'self'` y no necesita hash. Se acepta cualquier atributo en la etiqueta
 * (`type`, `nonce`, `defer`) porque Next los usa.
 */
const INLINE_SCRIPT = /<script(?![^>]*\bsrc\s*=)[^>]*>([\s\S]*?)<\/script\s*>/gi

/**
 * Hash de cada script en linea, en el formato que espera la CSP.
 *
 * Un script vacio no se incluye: no se ejecuta nada y un hash de cadena vacia
 * solo ensuciaria la cabecera.
 */
export function inlineScriptHashes(html: string): string[] {
  const hashes: string[] = []

  for (const match of html.matchAll(INLINE_SCRIPT)) {
    const body = match[1]
    if (body === undefined || body.length === 0) continue

    const digest = createHash('sha256').update(body, 'utf8').digest('base64')
    hashes.push(`'sha256-${digest}'`)
  }

  return hashes
}

/** La cabecera `Content-Security-Policy` para un HTML concreto. */
export function contentSecurityPolicy(html: string): string {
  const hashes = inlineScriptHashes(html)
  // `'self'` se mantiene siempre: los scripts con `src`, que son la mayoria, se
  // siguen sirviendo solo desde el propio esquema de la aplicacion.
  const scriptSrc =
    hashes.length > 0 ? `script-src 'self' ${hashes.join(' ')}` : "script-src 'self'"

  return [...BASE_DIRECTIVES, scriptSrc].join('; ')
}
