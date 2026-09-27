'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { SectionLabel, SegmentedControl } from '@superupscaly/ui'
import { SUPPORTED_SCALES, type ScaleFactor } from '@superupscaly/shared'
import { settingsAtom, translatorAtom, updateSettingsAtom } from '@/state/atoms/settings'

export function ScaleSelector() {
  const t = useAtomValue(translatorAtom)
  const settings = useAtomValue(settingsAtom)
  const update = useSetAtom(updateSettingsAtom)

  return (
    <div>
      <SectionLabel id="su-scale-label">{t('scale.title')}</SectionLabel>
      <SegmentedControl<ScaleFactor>
        labelId="su-scale-label"
        value={settings.scale}
        onChange={(value) => void update({ scale: value })}
        options={SUPPORTED_SCALES.map((value) => ({
          value,
          label: `${value}x`,
          ariaLabel: `${value}x`,
        }))}
      />
    </div>
  )
}
