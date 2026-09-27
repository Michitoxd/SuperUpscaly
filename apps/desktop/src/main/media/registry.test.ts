/**
 * Pruebas del registro de rutas que el renderer puede mostrar.
 *
 * Lo que se prueba aqui no es «se sirve una imagen»: es **quien decide que se
 * sirve**. La ruta de medios vive bajo el origen de la aplicacion, que la CSP ya
 * declara como propio, asi que una lista de autorizacion mal hecha convierte esa
 * misma CSP en un papel mojado. Un fallo de este modulo no se ve: se ve una
 * imagen, o ninguna, y ninguna de las dos cosas dice que se pueda leer el disco.
 *
 * Tambien se prueba el contrato de la URL, porque lo construye la interfaz y lo
 * interpreta el proceso principal: si el formato se separa, la comparacion sale
 * en blanco sin ningun error.
 */

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { isMediaPath, mediaUrl, parseMediaUrl } from '@superupscaly/shared'

import { clearMediaRegistry, isMediaAllowed, mediaRegistrySize, registerMediaPath } from './registry.ts'

// --- Quien puede mostrarse ---------------------------------------------------

test('una ruta no registrada no se sirve', () => {
  clearMediaRegistry()
  // Aunque tenga forma de ruta perfectamente valida: lo que autoriza no es la
  // forma, es haber pasado por el proceso principal.
  assert.equal(isMediaAllowed('/home/alguien/secreto.png'), false)
  assert.equal(mediaRegistrySize(), 0)
})

test('una ruta validada se sirve', () => {
  clearMediaRegistry()
  assert.equal(registerMediaPath('/home/alguien/foto.png'), true)
  assert.equal(isMediaAllowed('/home/alguien/foto.png'), true)
})

test('la coincidencia es exacta: no vale una ruta parecida', () => {
  clearMediaRegistry()
  registerMediaPath('/home/alguien/foto.png')

  assert.equal(isMediaAllowed('/home/alguien/foto.png'), true)
  assert.equal(isMediaAllowed('/home/alguien/foto.png.bak'), false)
  assert.equal(isMediaAllowed('/home/alguien/otra.png'), false)
  assert.equal(isMediaAllowed(''), false)
})

test('una ruta sin forma segura no se registra', () => {
  clearMediaRegistry()
  // El registro usa la misma puerta que el resto de la frontera IPC: lo que no
  // podria llegar al sistema de archivos tampoco puede llegar aqui.
  assert.equal(registerMediaPath(undefined), false)
  assert.equal(registerMediaPath(42), false)
  assert.equal(registerMediaPath(''), false)
  assert.equal(registerMediaPath('/home/alguien/con\u0000nul.png'), false)
  assert.equal(registerMediaPath('x'.repeat(5000)), false)
  assert.equal(mediaRegistrySize(), 0)
})

test('registrar dos veces la misma ruta no la duplica', () => {
  clearMediaRegistry()
  registerMediaPath('/home/alguien/foto.png')
  registerMediaPath('/home/alguien/foto.png')
  assert.equal(mediaRegistrySize(), 1)
})

// --- El contrato de la URL ---------------------------------------------------

test('la URL lleva la ruta y se puede recuperar entera', () => {
  clearMediaRegistry()
  // Espacios, acentos, almohadilla e interrogacion: los cuatro se rompen si la
  // ruta viaja sin codificar, y el resultado seria una peticion a otra ruta.
  const rutas = [
    '/home/alguien/foto.png',
    '/home/alguien/con espacios/foto escalada.png',
    '/home/alguien/año/niña #2.png',
    '/home/alguien/raro?a=1&b=2.png',
    'C:\\Users\\alguien\\foto.png',
  ]

  for (const ruta of rutas) {
    assert.equal(parseMediaUrl(mediaUrl(ruta)), ruta, `ida y vuelta de ${ruta}`)
  }
})

test('una URL que no es de medios no se interpreta como tal', () => {
  assert.equal(parseMediaUrl('app://superupscaly/index.html'), null)
  assert.equal(parseMediaUrl('app://superupscaly/media'), null)
  assert.equal(parseMediaUrl('app://superupscaly/media?p='), null)
  // Otro origen y otro esquema, aunque la ruta y el parametro coincidan.
  assert.equal(parseMediaUrl('app://otra/media?p=/etc/passwd'), null)
  assert.equal(parseMediaUrl('https://ejemplo.invalido/media?p=/etc/passwd'), null)
  assert.equal(parseMediaUrl('no es una url'), null)
})

test('solo la ruta exacta del servicio de imagenes es de medios', () => {
  assert.equal(isMediaPath('/media'), true)
  assert.equal(isMediaPath('/media/'), false)
  assert.equal(isMediaPath('/media/../../etc/passwd'), false)
  assert.equal(isMediaPath('/'), false)
})
