import { atom } from 'jotai'
import { queueAtom } from './queue'

/**
 * Item cuya comparacion antes y despues esta abierta, o `null`.
 *
 * Se guarda el **identificador** y no el item: el objeto de la cola se sustituye
 * entero cada vez que llega un evento del motor, asi que una copia guardada aqui
 * se quedaria con el progreso de hace un rato. Buscandolo por identificador, la
 * vista siempre lee el estado de ahora.
 */
export const compareItemIdAtom = atom<string | null>(null)

/**
 * Item que se esta comparando, ya resuelto contra la cola.
 *
 * Si el item desaparece de la cola —el usuario lo quita de la lista mientras la
 * comparacion esta abierta— devuelve `null` y la vista se cierra sola. Es la
 * razon de resolverlo aqui en lugar de guardar el item: cerrar una ventana que
 * enseña un archivo que ya no esta en la lista no deberia depender de que alguien
 * se acuerde de hacerlo.
 */
export const compareItemAtom = atom((get) => {
  const id = get(compareItemIdAtom)
  if (id === null) return null
  return get(queueAtom).find((item) => item.id === id) ?? null
})
