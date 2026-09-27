'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { cx } from '@superupscaly/ui'
import { LOCALES, type Locale } from '@superupscaly/shared'
import { localeAtom, translatorAtom, updateSettingsAtom } from '@/state/atoms/settings'

export function LanguageToggle() {
  const t = useAtomValue(translatorAtom)
  const locale = useAtomValue(localeAtom)
  const update = useSetAtom(updateSettingsAtom)

  return (
    <div className="flex items-center gap-1.5">
      <span className="text-[11px] text-su-text-muted/70">{t('settings.language')}</span>
      <div className="ml-auto flex gap-1">
        {LOCALES.map((value: Locale) => (
          <button
            key={value}
            type="button"
            aria-pressed={value === locale}
            onClick={() => void update({ locale: value })}
            className={cx(
              'rounded-md border px-2 py-0.5 text-[11px] transition-colors',
              'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover',
              value === locale
                ? 'border-su-accent bg-su-accent-muted text-su-text'
                : 'border-su-border text-su-text-muted hover:text-su-text',
            )}
          >
            {value.toUpperCase()}
          </button>
        ))}
      </div>
    </div>
  )
}
