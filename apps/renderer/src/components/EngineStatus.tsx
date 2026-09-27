'use client'

import { useAtomValue } from 'jotai'
import { cx } from '@superupscaly/ui'
import type { SidecarStatus, TranslationKey } from '@superupscaly/shared'
import { capabilitiesAtom, sidecarStatusAtom } from '@/state/atoms/sidecar'
import { translatorAtom } from '@/state/atoms/settings'
import { getBridge } from '@/lib/bridge'

/**
 * Estado del motor de escalado.
 *
 * Esta siempre visible, no solo cuando algo va mal. Un usuario que no entiende
 * por que el boton no hace nada necesita ver el estado del motor **antes** de
 * pulsarlo, no un aviso despues.
 */

const STATE_KEY: Record<SidecarStatus['state'], TranslationKey> = {
  stopped: 'sidecar.state.stopped',
  starting: 'sidecar.state.starting',
  ready: 'sidecar.state.ready',
  restarting: 'sidecar.state.restarting',
  failed: 'sidecar.state.failed',
  unavailable: 'sidecar.state.unavailable',
}

const DOT_COLOR: Record<SidecarStatus['state'], string> = {
  stopped: 'var(--color-su-text-muted)',
  starting: 'var(--color-su-accent-hover)',
  ready: 'var(--color-su-success)',
  restarting: 'var(--color-su-warning)',
  failed: 'var(--color-su-error)',
  unavailable: 'var(--color-su-warning)',
}

export function EngineStatus() {
  const t = useAtomValue(translatorAtom)
  const status = useAtomValue(sidecarStatusAtom)
  const capabilities = useAtomValue(capabilitiesAtom)

  const restart = (): void => {
    const bridge = getBridge()
    if (bridge) void bridge.restartSidecar()
  }

  const showRestart = status.state === 'failed' || status.state === 'unavailable'

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className={cx('h-2 w-2 shrink-0 rounded-full')}
          style={{ backgroundColor: DOT_COLOR[status.state] }}
        />
        <span className="text-[11px] text-su-text-muted">{t('sidecar.title')}</span>
        <span className="ml-auto text-[11px] text-su-text">{t(STATE_KEY[status.state])}</span>
      </div>

      {capabilities ? (
        // El motor **usado**, no el recomendado. Cuando no hay ONNX Runtime o
        // ningun modelo cargable, el motor es un interpolador clasico y el
        // usuario tiene que poder verlo antes de valorar el resultado.
        <div className="text-[10px] text-su-text-muted/70">
          {capabilities.engine} · {capabilities.cpu.physicalCores} {t('sidecar.cores')}
        </div>
      ) : null}

      {status.detail ? (
        <p className="text-[10px] leading-snug text-su-warning">{status.detail}</p>
      ) : null}

      {status.missingBinary ? (
        <p className="text-[10px] leading-snug text-su-text-muted/70">{t('sidecar.missingBinary')}</p>
      ) : null}

      {showRestart ? (
        <button
          type="button"
          onClick={restart}
          className="self-start text-[10px] text-su-text-muted/70 underline underline-offset-2 hover:text-su-text"
        >
          {t('sidecar.restart')}
        </button>
      ) : null}
    </div>
  )
}
