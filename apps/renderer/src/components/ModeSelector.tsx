'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { SectionLabel, cx } from '@superupscaly/ui'
import type { AppMode } from '@superupscaly/shared'
import { settingsAtom, translatorAtom, updateSettingsAtom } from '@/state/atoms/settings'

const MODE_ORDER: readonly AppMode[] = ['photo', 'illustration']

export function ModeSelector() {
  const t = useAtomValue(translatorAtom)
  const settings = useAtomValue(settingsAtom)
  const update = useSetAtom(updateSettingsAtom)

  return (
    <div>
      <SectionLabel id="su-mode-label">{t('mode.title')}</SectionLabel>
      <div role="radiogroup" aria-labelledby="su-mode-label" className="flex flex-col gap-1.5">
        {MODE_ORDER.map((value) => {
          const selected = value === settings.mode
          return (
            <button
              key={value}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => void update({ mode: value })}
              className={cx(
                'flex items-center gap-2.5 rounded-lg border px-3 py-2 text-left text-[13px] transition-colors duration-150',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover',
                selected
                  ? 'border-su-accent bg-su-accent-muted text-su-text'
                  : 'border-su-border bg-su-surface-2 text-su-text-muted hover:text-su-text',
              )}
            >
              <span
                aria-hidden="true"
                className={cx(
                  'h-2.5 w-2.5 shrink-0 rounded-full border',
                  selected ? 'border-su-text bg-su-text' : 'border-su-text-muted',
                )}
              />
              <span className="flex-1">
                {value === 'photo' ? t('mode.photo') : t('mode.illustration')}
              </span>
            </button>
          )
        })}
      </div>
      <p className="mt-2 text-[11px] leading-snug text-su-text-muted/80">{t('mode.hint')}</p>
    </div>
  )
}
