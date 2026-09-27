'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { appInfoAtom } from '@/state/atoms/app'
import { modelsAtom } from '@/state/atoms/sidecar'
import { modelsOpenAtom } from '@/state/atoms/models'
import { translatorAtom } from '@/state/atoms/settings'
import { AdvancedSettings } from './AdvancedSettings'
import { EngineStatus } from './EngineStatus'
import { LanguageToggle } from './LanguageToggle'
import { ModeSelector } from './ModeSelector'
import { ScaleSelector } from './ScaleSelector'

/**
 * Acceso al gestor de modelos.
 *
 * Lleva el numero de modelos sin descargar porque es la causa mas probable de que
 * el boton de escalar no haga nada: sin ese dato, el usuario tendria que abrir el
 * panel para enterarse de que le falta algo.
 */
function ModelsButton() {
  const t = useAtomValue(translatorAtom)
  const models = useAtomValue(modelsAtom)
  const setOpen = useSetAtom(modelsOpenAtom)

  const missing = models.filter((model) => model.state !== 'installed').length

  return (
    <button
      type="button"
      onClick={() => setOpen(true)}
      className="mt-2 flex w-full items-center gap-2 text-left text-[11px] text-su-text-muted hover:text-su-text"
    >
      <span>{t('models.open')}</span>
      {models.length > 0 ? (
        <span className="ml-auto text-[10px] text-su-text-muted/70">
          {missing === 0 ? t('models.state.installed') : t('models.missingCount', { count: missing })}
        </span>
      ) : null}
    </button>
  )
}

export function Sidebar() {
  const t = useAtomValue(translatorAtom)
  const appInfo = useAtomValue(appInfoAtom)

  return (
    <aside className="flex h-full w-[272px] shrink-0 flex-col border-r border-su-border bg-su-surface">
      <div className="flex items-center gap-2.5 px-4 pt-4 pb-3">
        <span
          aria-hidden="true"
          className="flex h-7 w-7 items-center justify-center rounded-lg bg-su-accent text-[13px] font-medium text-white"
        >
          S
        </span>
        <div className="min-w-0">
          <div className="truncate text-[14px] font-medium text-su-text">{t('app.name')}</div>
          <div className="truncate text-[11px] text-su-text-muted">{t('app.tagline')}</div>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 pb-4">
        <ModeSelector />
        <ScaleSelector />
        <AdvancedSettings />
      </div>

      <div className="border-t border-su-border px-4 py-3">
        <EngineStatus />
        <ModelsButton />
        <div className="mt-3 border-t border-su-border/60 pt-3">
          <LanguageToggle />
        </div>
        <div className="mt-2 flex items-center justify-between text-[10px] text-su-text-muted/60">
          <span>v{appInfo?.version ?? '0.1.0'}</span>
        </div>
      </div>
    </aside>
  )
}
