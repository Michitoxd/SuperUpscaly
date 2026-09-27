/**
 * Este archivo contenia el simulador de progreso de la Fase 1.
 *
 * Se retiro al conectar la interfaz con el sidecar, y conviene dejar constancia
 * del motivo porque la tentacion de recuperarlo es real: permitia desarrollar la
 * interfaz sin compilar nada.
 *
 * El problema es lo que hacia cuando faltaba el motor: mostraba una barra de
 * progreso avanzando y un resumen de "N imagenes completadas" sin que se hubiera
 * procesado ninguna. La carpeta de salida se quedaba vacia y nada explicaba por
 * que. Es exactamente el fallo silencioso que el proyecto se propone eliminar.
 *
 * Lo que lo sustituye:
 *
 * - `state/useRun.ts` envia un trabajo real y, si el motor no esta disponible,
 *   lo dice con el motivo concreto (incluido el comando para compilarlo).
 * - `state/useSidecarEvents.ts` traduce los eventos del sidecar en estado de la
 *   interfaz. Los items se marcan como terminados porque el motor lo dice, no
 *   porque la interfaz lo suponga.
 * - `components/EngineStatus.tsx` muestra el estado del motor de forma
 *   permanente, para que el usuario lo vea antes de pulsar el boton.
 *
 * Para desarrollar la interfaz sin sidecar, el camino correcto no es simular el
 * progreso: es compilar el sidecar (`cargo build --release -p su-cli`), que
 * arranca en menos de un segundo y funciona con el backend de referencia sin
 * necesidad de modelos ni GPU.
 */

export {}
