'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import { mediaUrl, type Translator } from '@superupscaly/shared'
import { Button } from '@superupscaly/ui'
import { compareItemAtom, compareItemIdAtom } from '@/state/atoms/compare'
import { settingsAtom, translatorAtom } from '@/state/atoms/settings'
import { getBridge } from '@/lib/bridge'
import { formatDuration } from '@/lib/format'

/**
 * Comparacion antes y despues.
 *
 * ## Que problema resuelve
 *
 * Un escalado se juzga comparando, y hasta ahora la unica forma de hacerlo era
 * salir a buscar el resultado al explorador de archivos y ponerlo al lado del
 * original en otro programa. Con el resultado y la entrada ya en la cola, la
 * comparacion es una linea que se mueve.
 *
 * ## Por que la linea se puede arrastrar en toda la imagen
 *
 * El gesto es arrastrar en cualquier punto, no solo sobre el tirador: ir a buscar
 * el tirador con el raton cada vez que se quiere mirar el borde de un objeto es el
 * tipo de friccion que hace que la herramienta no se use. El tirador sigue
 * existiendo —y es el que recibe el foco del teclado— porque hay que poder mover la
 * linea sin raton.
 *
 * ## Las dos imagenes son la misma caja
 *
 * Original y resultado se dibujan en el mismo rectangulo con `object-contain`, y
 * la linea no recorta una imagen: recorta la capa del resultado. Asi las dos
 * mitades quedan siempre alineadas pixel con pixel a la misma escala, que es lo
 * unico que hace util una comparacion: si cada imagen se ajustara por su cuenta al
 * ancho, la linea estaria comparando dos zoom distintos.
 *
 * ## La linea recorre la imagen, no el marco
 *
 * Una imagen no llena el marco: le sobra franja a los lados o arriba y abajo. Si el
 * recorrido de la linea fuera el del marco, en los extremos se saldria de la imagen
 * y el tirador quedaria flotando sobre el fondo. Por eso el recorrido se calcula
 * sobre el rectangulo que la imagen ocupa de verdad, y de ahi sale el porcentaje que
 * se guarda: 0 % es el borde izquierdo de la imagen y 100 %, el derecho.
 */

/** Posicion inicial de la linea, en porcentaje. */
const START = 50
const STEP = 2
const PAGE_STEP = 10

/** Separacion de las etiquetas respecto al borde de la imagen, en pixeles. */
const LABEL_INSET = 8

interface Size {
  width: number
  height: number
}

function clamp(value: number): number {
  return Math.min(100, Math.max(0, value))
}

function readSize(image: HTMLImageElement): Size | null {
  // `naturalWidth` es 0 mientras la imagen no ha terminado de decodificarse.
  if (image.naturalWidth <= 0 || image.naturalHeight <= 0) return null
  return { width: image.naturalWidth, height: image.naturalHeight }
}

function sizeLabel(size: Size | null, t: Translator): string {
  if (!size) return ''
  return t('compare.dimensions', { width: size.width, height: size.height })
}

/**
 * Factor de escala deducido de las dos medidas.
 *
 * Se prefiere medirlo a leerlo del ajuste: el ajuste dice con que se escalaria
 * **ahora**, y entre el lote y la comparacion puede haber cambiado. Lo que el usuario
 * esta viendo es la proporcion entre las dos imagenes que tiene delante.
 */
function measuredScale(original: Size | null, upscaled: Size | null): string | null {
  if (!original || !upscaled || original.width <= 0) return null
  const ratio = upscaled.width / original.width
  if (!Number.isFinite(ratio) || ratio <= 0) return null
  return Number.isInteger(ratio) ? `${ratio}x` : `${ratio.toFixed(1)}x`
}

/**
 * Rectangulo que ocupa la imagen dentro del marco, con `object-contain`.
 *
 * Devuelve la franja vacia de la izquierda y el ancho visible: es todo lo que hace
 * falta para pasar de una coordenada del raton a un porcentaje de la imagen y para
 * colocar la linea encima.
 */
interface Rect {
  left: number
  top: number
  width: number
  height: number
}

function containedRect(frame: Size, image: Size): Rect {
  const scale = Math.min(frame.width / image.width, frame.height / image.height)
  const width = image.width * scale
  const height = image.height * scale
  return { left: (frame.width - width) / 2, top: (frame.height - height) / 2, width, height }
}

export function CompareView() {
  const t = useAtomValue(translatorAtom)
  const settings = useAtomValue(settingsAtom)
  const item = useAtomValue(compareItemAtom)
  const setItemId = useSetAtom(compareItemIdAtom)

  const stageRef = useRef<HTMLDivElement | null>(null)
  const draggingRef = useRef(false)

  const [stageSize, setStageSize] = useState<Size | null>(null)
  const [position, setPosition] = useState(START)
  const [originalSize, setOriginalSize] = useState<Size | null>(null)
  const [upscaledSize, setUpscaledSize] = useState<Size | null>(null)
  const [failed, setFailed] = useState<'original' | 'upscaled' | null>(null)

  const itemId = item?.id ?? null

  const close = useCallback((): void => setItemId(null), [setItemId])

  // Al cambiar de imagen la linea vuelve al centro: una posicion heredada de la
  // comparacion anterior no significa nada en esta.
  useEffect(() => {
    setPosition(START)
    setOriginalSize(null)
    setUpscaledSize(null)
    setFailed(null)
  }, [itemId])

  useEffect(() => {
    if (itemId === null) return
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [itemId, close])

  // El marco cambia de tamano al redimensionar la ventana, y el recorrido de la
  // linea depende de él: sin observarlo, la linea quedaria midiendo el marco viejo.
  useEffect(() => {
    const stage = stageRef.current
    if (!stage) return
    const measure = (): void => {
      const rect = stage.getBoundingClientRect()
      if (rect.width > 0 && rect.height > 0) setStageSize({ width: rect.width, height: rect.height })
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(stage)
    return () => observer.disconnect()
  }, [itemId])

  // El recorrido de la linea es el rectangulo que ocupa la imagen dentro del marco,
  // no el marco entero: una imagen no suele llenarlo, y midiendo el marco la linea
  // se saldria de ella por los extremos. Mientras no se sabe el tamano de la imagen
  // (las dos se estan decodificando) se cae al marco, que es el caso de una imagen
  // que si lo llena.
  const imageBox = upscaledSize ?? originalSize
  const visible = stageSize && imageBox ? containedRect(stageSize, imageBox) : null
  const visibleLeft = visible?.left ?? null
  const visibleWidth = visible?.width ?? null

  const moveTo = useCallback(
    (clientX: number): void => {
      const stage = stageRef.current
      if (!stage) return
      const frame = stage.getBoundingClientRect()
      const width = visibleWidth ?? frame.width
      const offset = visibleLeft ?? 0
      if (width <= 0) return
      setPosition(clamp(((clientX - frame.left - offset) / width) * 100))
    },
    [visibleLeft, visibleWidth],
  )

  const onHandleKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>): void => {
    const jump = event.shiftKey ? PAGE_STEP : STEP
    switch (event.key) {
      case 'ArrowLeft':
        setPosition((current) => clamp(current - jump))
        break
      case 'ArrowRight':
        setPosition((current) => clamp(current + jump))
        break
      case 'PageDown':
        setPosition((current) => clamp(current - PAGE_STEP))
        break
      case 'PageUp':
        setPosition((current) => clamp(current + PAGE_STEP))
        break
      case 'Home':
        setPosition(0)
        break
      case 'End':
        setPosition(100)
        break
      default:
        return
    }
    event.preventDefault()
  }, [])

  // Sin item no hay nada que comparar. Esto tambien cubre el caso de que el
  // usuario quite de la cola la imagen que estaba mirando: la vista se cierra.
  if (!item || !item.outPath) return null

  const originalUrl = mediaUrl(item.path)
  const upscaledUrl = mediaUrl(item.outPath)
  const showOriginal = failed !== 'original'
  const showUpscaled = failed !== 'upscaled'
  const comparable = showOriginal && showUpscaled

  const openFolder = (): void => {
    if (item.outPath) void getBridge()?.revealInFolder(item.outPath)
  }

  // La posicion guardada es un porcentaje **de la imagen**, y son dos cosas
  // distintas las que se colocan con ella:
  //
  //  - la linea y el tirador, en pixeles, dentro del marco;
  //  - el recorte de la capa del resultado, que `clip-path` mide en porcentaje del
  //    **marco**, no de la imagen.
  //
  // Confundirlas desplaza la costura justo donde esta la franja vacia, que es donde
  // no se nota hasta que se compara una imagen con otra proporcion.
  const dividerLeft =
    visibleLeft !== null && visibleWidth !== null ? visibleLeft + (position / 100) * visibleWidth : null
  const clipPercent =
    dividerLeft !== null && stageSize ? (dividerLeft / stageSize.width) * 100 : position

  const originalLabelStyle =
    visibleLeft !== null ? { left: `${visibleLeft + LABEL_INSET}px` } : undefined
  const upscaledLabelStyle =
    visibleLeft !== null && visibleWidth !== null && stageSize
      ? { right: `${stageSize.width - visibleLeft - visibleWidth + LABEL_INSET}px` }
      : undefined
  // La linea y el tirador comparten la coordenada horizontal y nada mas: la linea
  // es tan alta como la imagen, y el tirador es un circulo de tamano fijo centrado en
  // ella. Darle a los dos el mismo `style` convierte el tirador en una barra que
  // sobresale por arriba y por abajo de la imagen.
  const dividerLineStyle = visible
    ? { left: `${dividerLeft}px`, top: `${visible.top}px`, height: `${visible.height}px` }
    : { left: `${position}%`, top: 0, height: '100%' }

  const handleStyle = { left: dividerLeft !== null ? `${dividerLeft}px` : `${position}%` }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('compare.title')}
      className="absolute inset-0 z-40 flex flex-col bg-su-base/95 p-4"
    >
      <div className="flex min-h-0 flex-1 flex-col gap-3 rounded-xl border border-su-border bg-su-surface p-3">
        <header className="flex shrink-0 items-center gap-2">
          <h2 className="shrink-0 text-[14px] font-medium text-su-text">{t('compare.title')}</h2>
          <span className="min-w-0 truncate text-[11px] text-su-text-muted" title={item.path}>
            {item.name}
          </span>
          <span className="ml-auto flex shrink-0 items-center gap-2">
            <Button variant="ghost" size="sm" onClick={close}>
              {t('common.close')}
            </Button>
          </span>
        </header>

        <div
          ref={stageRef}
          onPointerDown={(event) => {
            // Se mueve la linea al punto pulsado y se captura el puntero: asi el
            // arrastre sigue funcionando aunque el cursor salga de la imagen, que es
            // lo que pasa constantemente al llevarlo de un borde al otro.
            event.preventDefault()
            draggingRef.current = true
            event.currentTarget.setPointerCapture(event.pointerId)
            moveTo(event.clientX)
          }}
          onPointerMove={(event) => {
            if (draggingRef.current) moveTo(event.clientX)
          }}
          onPointerUp={(event) => {
            draggingRef.current = false
            // `pointercancel` y la perdida de foco sueltan la captura por su cuenta:
            // liberarla sin comprobarlo lanzaria al responder a un gesto terminado.
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              event.currentTarget.releasePointerCapture(event.pointerId)
            }
          }}
          onPointerCancel={() => {
            draggingRef.current = false
          }}
          className="su-checker relative min-h-0 flex-1 touch-none select-none overflow-hidden rounded-lg border border-su-border"
        >
          {showOriginal ? (
            <img
              src={originalUrl}
              alt={`${t('compare.original')} ${item.name}`}
              draggable={false}
              onLoad={(event) => setOriginalSize(readSize(event.currentTarget))}
              onError={() => setFailed('original')}
              className="pointer-events-none absolute inset-0 h-full w-full object-contain"
            />
          ) : null}

          {showUpscaled ? (
            // La capa del resultado es la que se recorta: la mitad izquierda enseña
            // el original y la derecha, lo que produjo el motor.
            <div
              className="pointer-events-none absolute inset-0"
              style={comparable ? { clipPath: `inset(0 0 0 ${clipPercent}%)` } : undefined}
            >
              <img
                src={upscaledUrl}
                alt={`${t('compare.upscaled')} ${item.name}`}
                draggable={false}
                onLoad={(event) => setUpscaledSize(readSize(event.currentTarget))}
                onError={() => setFailed('upscaled')}
                className="absolute inset-0 h-full w-full object-contain"
              />
            </div>
          ) : null}

          {showOriginal ? (
            <span
              style={originalLabelStyle}
              className="pointer-events-none absolute left-2 top-2 max-w-[45%] truncate rounded-md border border-su-border bg-su-base/85 px-2 py-1 text-[10px] text-su-text-muted"
            >
              {t('compare.original')}
              {originalSize ? ` · ${sizeLabel(originalSize, t)}` : ''}
            </span>
          ) : null}

          {showUpscaled ? (
            <span
              style={upscaledLabelStyle}
              className="pointer-events-none absolute right-2 top-2 max-w-[45%] truncate rounded-md border border-su-border bg-su-base/85 px-2 py-1 text-[10px] text-su-text-muted"
            >
              {t('compare.upscaled')} · {measuredScale(originalSize, upscaledSize) ?? `${settings.scale}x`}
              {upscaledSize ? ` · ${sizeLabel(upscaledSize, t)}` : ''}
            </span>
          ) : null}

          {comparable ? (
            <>
              <div
                aria-hidden="true"
                className="pointer-events-none absolute w-px bg-su-accent-hover"
                style={dividerLineStyle}
              />
              <div
                role="slider"
                tabIndex={0}
                aria-label={t('compare.divider')}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(position)}
                aria-valuetext={`${Math.round(position)}%`}
                onKeyDown={onHandleKeyDown}
                style={handleStyle}
                className="absolute top-1/2 flex h-8 w-8 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border border-su-accent-hover bg-su-accent text-[13px] leading-none text-white shadow-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover"
              >
                <span aria-hidden="true">&#8596;</span>
              </div>
            </>
          ) : (
            // Un formato que la ventana no sabe dibujar (TIFF, BMP) no se puede
            // comparar, y decirlo es la diferencia entre un hueco y una explicacion.
            <div className="pointer-events-none absolute inset-x-0 bottom-2 mx-auto w-fit max-w-[90%] rounded-md border border-su-warning/40 bg-su-base/90 px-3 py-1.5 text-center text-[11px] text-su-warning">
              {failed === 'original' ? t('compare.failedOriginal') : t('compare.failedUpscaled')}{' '}
              {t('compare.failedHint')}
            </div>
          )}
        </div>

        <footer className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1">
          {comparable ? (
            <span className="text-[11px] text-su-text-muted">{t('compare.hint')}</span>
          ) : null}

          {item.notes && item.notes.length > 0 ? (
            <span
              className="rounded-md border border-su-warning/40 bg-su-warning/10 px-1.5 py-0.5 text-[10px] text-su-warning"
              title={item.notes.join('\n')}
            >
              {t('summary.notes')}: {item.notes.join(' · ')}
            </span>
          ) : null}

          <span className="ml-auto flex shrink-0 items-center gap-2">
            {item.durationMs !== undefined ? (
              <span className="text-[11px] tabular-nums text-su-text-muted">
                {formatDuration(item.durationMs)}
              </span>
            ) : null}
            <Button variant="ghost" size="sm" onClick={openFolder}>
              {t('compare.openFolder')}
            </Button>
          </span>
        </footer>
      </div>
    </div>
  )
}
