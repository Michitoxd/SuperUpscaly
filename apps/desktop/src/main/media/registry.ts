import { isSafePathString } from '@superupscaly/shared'

/**
 * Rutas que el renderer tiene permitido mostrar.
 *
 * ## Por que una lista y no un comprobador de rutas
 *
 * Servir imagenes locales por el esquema de la aplicacion abre una pregunta
 * incomoda: si el renderer puede pedir cualquier ruta, la CSP deja de proteger
 * nada y una inyeccion en la pagina se lleva por delante el disco entero.
 *
 * La respuesta de este modulo es que el renderer **solo puede pedir lo que ya
 * conoce**: las rutas se anaden aqui desde el proceso principal en los dos
 * unicos momentos en que una imagen pasa a interesar al usuario —
 *
 * 1. cuando termina de validarse (es una imagen de la cola), y
 * 2. cuando el motor informa de que acaba de escribir un resultado.
 *
 * No hay forma de que el renderer anada una entrada: la lista no se expone por
 * IPC. Una ruta inventada no coincide con ninguna entrada y se rechaza.
 *
 * ## Por que coincidencia exacta y sin canonicalizar
 *
 * Los dos lados usan la misma cadena: el renderer pide la ruta que el mismo
 * recibio, y aqui se guarda tal cual llego. Canonicalizar otra vez (resolviendo
 * enlaces) podria convertir una ruta valida en otra distinta de la que viaja en
 * la URL, y entonces una imagen legitima dejaria de mostrarse. La validacion de
 * forma la hace `isSafePathString`, que es la misma puerta que usa el resto de la
 * frontera IPC.
 */

const allowed = new Set<string>()

/**
 * Autoriza una ruta. Devuelve `false` si no tiene forma de ruta segura, para que
 * el llamador pueda dejar constancia en lugar de dar por hecho que se registro.
 */
export function registerMediaPath(absolutePath: unknown): boolean {
  if (!isSafePathString(absolutePath)) return false
  allowed.add(absolutePath)
  return true
}

/** true si la ruta esta autorizada. */
export function isMediaAllowed(absolutePath: string): boolean {
  return allowed.has(absolutePath)
}

/** Cuantas rutas hay autorizadas. Solo para el registro de la aplicacion. */
export function mediaRegistrySize(): number {
  return allowed.size
}

/** Vacia el registro. Existe para las pruebas: en produccion no se vacia nunca. */
export function clearMediaRegistry(): void {
  allowed.clear()
}
