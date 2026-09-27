/**
 * Pruebas de la CSP del renderer.
 *
 * El fallo que estas pruebas existen para evitar no se ve: si la cabecera no deja
 * ejecutar los scripts en linea del export de Next, la ventana sigue mostrandose
 * —con su tema, sus botones y su texto— pero no se hidrata, asi que nada responde
 * y no hay ningun error visible. Se comprueba, por tanto, la propiedad que lo
 * evita: que los hashes cubren exactamente los scripts en linea del HTML servido.
 *
 * El vector del hash no se calcula con la misma funcion que se prueba: se usa un
 * valor conocido de `sha256("abc")`, que es el mismo criterio que sigue el
 * manifiesto de modelos con su vector conocido.
 */

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { contentSecurityPolicy, inlineScriptHashes } from './csp.ts'

/** Valor publicado de `base64(sha256("abc"))`. */
const SHA256_DE_ABC = 'ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0='

/** HTML con la forma que tiene el export real de Next. */
const HTML_DE_NEXT = [
  '<!DOCTYPE html><html><head>',
  '<script src="/_next/static/chunks/app.js"></script>',
  '<script>(self.__next_f=self.__next_f||[]).push([0])</script>',
  '</head><body><div id="__next"></div></body></html>',
].join('')

test('el hash de un script en linea es el que dice el estandar', () => {
  const hashes = inlineScriptHashes('<script>abc</script>')
  assert.deepEqual(hashes, [`'sha256-${SHA256_DE_ABC}'`])
})

test('la cabecera incluye el hash de cada script en linea', () => {
  const policy = contentSecurityPolicy(HTML_DE_NEXT)

  assert.match(policy, /script-src 'self' 'sha256-[A-Za-z0-9+/=]+'/)
  // El bootstrap de Next tiene que quedar cubierto: es el que arranca la
  // hidratacion, y es el que bloqueaba la CSP anterior.
  assert.equal(inlineScriptHashes(HTML_DE_NEXT).length, 1)
})

test('un script con src no necesita hash', () => {
  // Lo cubre `'self'`: añadirlo obligaria a recalcular la cabecera cada vez que
  // Next renombra un chunk, sin ninguna ganancia.
  assert.deepEqual(inlineScriptHashes('<script src="/_next/app.js"></script>'), [])
})

test('la cabecera nunca permite scripts en linea a lo ancho', () => {
  // La tentacion es resolver esto con `'unsafe-inline'`. Eso dejaria el
  // `script-src` sin efecto y convertiria la CSP en decoracion.
  const policy = contentSecurityPolicy(HTML_DE_NEXT)
  assert.ok(!policy.includes('unsafe-inline;'), policy)
  assert.equal(policy.includes("script-src 'unsafe-inline'"), false, policy)
  // Estilos en linea si se permiten: Next inyecta los criticos, y un estilo no
  // puede exfiltrar nada por si mismo.
  assert.ok(policy.includes("style-src 'self' 'unsafe-inline'"), policy)
})

test('sin scripts en linea la politica no lleva hashes', () => {
  const policy = contentSecurityPolicy('<html><body>sin scripts</body></html>')
  assert.ok(policy.includes("script-src 'self'"), policy)
  assert.equal(policy.includes('sha256-'), false, policy)
})

test('un script en linea vacio no aporta un hash de cadena vacia', () => {
  // Un hash de la cadena vacia no protege nada y solo ensucia la cabecera.
  assert.deepEqual(inlineScriptHashes('<script></script>'), [])
})

test('varios scripts en linea producen varios hashes, en orden', () => {
  const html = '<script>uno</script><script>dos</script>'
  const hashes = inlineScriptHashes(html)

  assert.equal(hashes.length, 2)
  assert.notEqual(hashes[0], hashes[1])
  // El orden es el del documento, para que la cabecera se lea igual que el HTML.
  assert.equal(hashes[0], inlineScriptHashes('<script>uno</script>')[0])
})

test('los atributos de la etiqueta no cambian el hash', () => {
  // La CSP cubre el **contenido** del script, no su etiqueta. Confundirlo haria
  // que un `nonce` o un `type` anadido por Next dejara la ventana muda.
  const simple = inlineScriptHashes('<script>abc</script>')
  const conAtributos = inlineScriptHashes('<script type="text/javascript" defer>abc</script>')

  assert.deepEqual(conAtributos, simple)
})

test('la politica conserva las demas directivas', () => {
  const policy = contentSecurityPolicy(HTML_DE_NEXT)

  for (const directive of [
    "default-src 'self'",
    "img-src 'self' data: blob:",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ]) {
    assert.ok(policy.includes(directive), `${directive} falta en ${policy}`)
  }
})
