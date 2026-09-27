'use client'

import { useAtomValue, useSetAtom } from 'jotai'
import { Field, SectionLabel, SegmentedControl, Switch, cx } from '@superupscaly/ui'
import type { TranslationKey } from '@superupscaly/shared'
import { settingsAtom, translatorAtom, updateSettingsAtom } from '@/state/atoms/settings'
import { pipelinesAtom, sidecarReadyAtom } from '@/state/atoms/sidecar'
import { pipelineFor, planStages } from '@/lib/pipelinePlan'

const TILE_OPTIONS = [
  { value: 'auto' as const, label: 'Auto' },
  { value: 256 as const, label: '256' },
  { value: 384 as const, label: '384' },
  { value: 512 as const, label: '512' },
  { value: 768 as const, label: '768' },
  { value: 1024 as const, label: '1024' },
]

export function AdvancedSettings() {
  const t = useAtomValue(translatorAtom)
  const settings = useAtomValue(settingsAtom)
  // El aviso se deriva del estado **vivo** del motor, no de un campo fijado al
  // arrancar: el sidecar tarda en estar listo y un dato congelado mentiria
  // justo durante los primeros segundos, que es cuando el usuario mira.
  const engineReady = useAtomValue(sidecarReadyAtom)
  const update = useSetAtom(updateSettingsAtom)

  // La cadena se dibuja a partir de los pipelines del motor, no de una copia en
  // cliente: el motor es el que decide el modelo de cada etapa.
  const pipelines = useAtomValue(pipelinesAtom)
  const chain = planStages(pipelineFor(pipelines, settings.mode, settings.scale))
  const showQuality = settings.outputFormat !== 'png'

  return (
    <div className="border-t border-su-border pt-3">
      <button
        type="button"
        aria-expanded={settings.advancedOpen}
        onClick={() => void update({ advancedOpen: !settings.advancedOpen })}
        className="flex w-full items-center gap-2 rounded-lg px-1 py-1.5 text-left text-[12px] font-medium text-su-text-muted transition-colors hover:text-su-text focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-su-accent-hover"
      >
        <span
          aria-hidden="true"
          className={cx('inline-block transition-transform duration-150', settings.advancedOpen && 'rotate-90')}
        >
          ▸
        </span>
        {t('advanced.title')}
      </button>

      {settings.advancedOpen ? (
        <div className="mt-3 flex flex-col gap-4">
          {engineReady ? null : (
            <p className="rounded-lg border border-su-warning/40 bg-su-warning/10 px-2.5 py-2 text-[11px] leading-snug text-su-warning">
              {t('advanced.noBackend')}
            </p>
          )}

          {/* --- Cadena de modelos --- */}
          <div className="flex flex-col gap-2">
            <SectionLabel className="mb-0">{t('advanced.models.label')}</SectionLabel>
            <SegmentedControl
              value={settings.modelChainMode}
              onChange={(value) => void update({ modelChainMode: value })}
              options={[
                { value: 'auto' as const, label: t('common.auto') },
                { value: 'manual' as const, label: t('advanced.models.manual') },
              ]}
            />
            <ul className="flex flex-wrap gap-1.5">
              {chain.map((entry, index) => (
                <li
                  key={`${entry.stage}-${index}`}
                  className="rounded-md border border-su-border bg-su-surface-2 px-2 py-1 text-[11px] text-su-text-muted"
                  title={entry.model ?? undefined}
                >
                  {t(`progress.stage.${entry.stage}` as TranslationKey)}
                  {/* Una etapa condicional puede no llegar a ejecutarse: se marca
                      en vez de prometerla, porque quien lo decide es el analisis. */}
                  {entry.conditional ? ' \u00b7?' : ''}
                  {entry.model ? <span className="text-su-text"> · {entry.model}</span> : null}
                </li>
              ))}
            </ul>
            <p className="text-[11px] leading-snug text-su-text-muted/80">{t('advanced.models.hint')}</p>
          </div>

          {/* --- Formato de salida --- */}
          <div className="flex flex-col gap-2">
            <SectionLabel className="mb-0">{t('advanced.format.label')}</SectionLabel>
            <SegmentedControl
              value={settings.outputFormat}
              onChange={(value) => void update({ outputFormat: value })}
              options={[
                { value: 'png' as const, label: 'PNG' },
                { value: 'jpg' as const, label: 'JPG' },
                { value: 'webp' as const, label: 'WEBP' },
              ]}
            />
            {showQuality ? (
              <Field label={`${t('advanced.format.quality')} · ${settings.outputQuality}`} htmlFor="su-quality">
                <input
                  id="su-quality"
                  type="range"
                  min={1}
                  max={100}
                  value={settings.outputQuality}
                  onChange={(event) => void update({ outputQuality: Number(event.target.value) })}
                  className="w-full accent-su-accent"
                />
              </Field>
            ) : null}
            <Field label={t('advanced.format.suffix')} htmlFor="su-suffix">
              <input
                id="su-suffix"
                type="text"
                maxLength={32}
                value={settings.suffix}
                onChange={(event) => void update({ suffix: event.target.value })}
                className="h-9 w-full rounded-lg border border-su-border bg-su-surface-2 px-3 text-[13px] text-su-text outline-none focus:border-su-accent-hover"
              />
            </Field>
            <Field label={t('advanced.format.preserveMetadata')} inline>
              <Switch
                checked={settings.preserveMetadata}
                onChange={(checked) => void update({ preserveMetadata: checked })}
                label={t('advanced.format.preserveMetadata')}
              />
            </Field>
          </div>

          {/* --- Rendimiento --- */}
          <div className="flex flex-col gap-3">
            <SectionLabel className="mb-0">{t('advanced.tile.label')}</SectionLabel>
            <SegmentedControl
              value={settings.tileSize}
              onChange={(value) => void update({ tileSize: value })}
              options={TILE_OPTIONS}
              size="sm"
            />
            <p className="text-[11px] leading-snug text-su-text-muted/80">{t('advanced.tile.hint')}</p>

            <Field label={t('advanced.device.label')} hint={t('advanced.device.hint')}>
              <SegmentedControl
                value={settings.device}
                onChange={(value) => void update({ device: value })}
                options={[
                  { value: 'auto' as const, label: t('common.auto') },
                  { value: 'gpu' as const, label: t('advanced.device.gpu') },
                  { value: 'cpu' as const, label: t('advanced.device.cpu') },
                ]}
                size="sm"
              />
            </Field>

            <Field
              label={`${t('advanced.concurrency.label')} · ${settings.concurrency}`}
              hint={t('advanced.concurrency.hint')}
              htmlFor="su-concurrency"
            >
              <input
                id="su-concurrency"
                type="range"
                min={1}
                max={4}
                value={settings.concurrency}
                onChange={(event) => void update({ concurrency: Number(event.target.value) })}
                className="w-full accent-su-accent"
              />
            </Field>

            <Field label={t('advanced.unload.label')} hint={t('advanced.unload.hint')} inline>
              <Switch
                checked={settings.unloadBetweenImages}
                onChange={(checked) => void update({ unloadBetweenImages: checked })}
                label={t('advanced.unload.label')}
              />
            </Field>
          </div>

          {/* --- Procesado --- */}
          <div className="flex flex-col gap-3">
            <Field label={t('advanced.denoise.label')} hint={t('advanced.denoise.hint')}>
              <SegmentedControl
                value={settings.denoise}
                onChange={(value) => void update({ denoise: value })}
                options={[
                  { value: 'off' as const, label: t('common.off') },
                  { value: 'auto' as const, label: t('common.auto') },
                  { value: 'on' as const, label: t('common.on') },
                ]}
                size="sm"
              />
            </Field>

            <Field
              label={t('advanced.face.label')}
              hint={settings.mode === 'photo' ? t('advanced.face.hint') : t('advanced.face.photoOnly')}
              note={settings.mode === 'photo' ? undefined : t('advanced.face.photoOnly')}
            >
              <SegmentedControl
                value={settings.faceRestore}
                onChange={(value) => void update({ faceRestore: value })}
                options={[
                  { value: 'off' as const, label: t('common.off'), disabled: settings.mode !== 'photo' },
                  { value: 'auto' as const, label: t('common.auto'), disabled: settings.mode !== 'photo' },
                  { value: 'low' as const, label: t('advanced.face.low'), disabled: settings.mode !== 'photo' },
                  { value: 'medium' as const, label: t('advanced.face.medium'), disabled: settings.mode !== 'photo' },
                  { value: 'high' as const, label: t('advanced.face.high'), disabled: settings.mode !== 'photo' },
                ]}
                size="sm"
              />
            </Field>

            <Field label={t('advanced.sharpen.label')} hint={t('advanced.sharpen.hint')} inline>
              <Switch
                checked={settings.sharpen}
                onChange={(checked) => void update({ sharpen: checked })}
                label={t('advanced.sharpen.label')}
              />
            </Field>
          </div>
        </div>
      ) : (
        <p className="mt-2 text-[11px] leading-snug text-su-text-muted/70">{t('advanced.collapsedHint')}</p>
      )}
    </div>
  )
}
