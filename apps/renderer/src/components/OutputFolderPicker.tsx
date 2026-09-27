'use client'

import { useCallback } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import { Button } from '@superupscaly/ui'
import { defaultOutputDirAtom, outputDirAtom, outputDirStatusAtom } from '@/state/atoms/app'
import { translatorAtom, updateSettingsAtom } from '@/state/atoms/settings'
import { pushToastAtom } from '@/state/atoms/ui'
import { getBridge } from '@/lib/bridge'
import { shortenPath } from '@/lib/format'

export function OutputFolderPicker() {
  const t = useAtomValue(translatorAtom)
  const outputDir = useAtomValue(outputDirAtom)
  const systemDefault = useAtomValue(defaultOutputDirAtom)
  const status = useAtomValue(outputDirStatusAtom)
  const setOutputDir = useSetAtom(outputDirAtom)
  const setStatus = useSetAtom(outputDirStatusAtom)
  const update = useSetAtom(updateSettingsAtom)
  const pushToast = useSetAtom(pushToastAtom)

  const isCustom = systemDefault.length > 0 && outputDir !== systemDefault
  const invalid = status === 'invalid'

  const choose = useCallback(async (): Promise<void> => {
    const bridge = getBridge()
    if (!bridge) {
      pushToast({ tone: 'error', message: t('noBridge.body') })
      return
    }

    const picked = await bridge.pickFolder(t('output.pickTitle'))
    if (!picked) return

    const ensured = await bridge.ensureDir(picked)
    if (!ensured.ok) {
      setStatus('invalid')
      pushToast({
        tone: 'error',
        message: t('output.notWritable'),
        code: 'SU-E150',
        action: t('errors.SU-E150.action'),
      })
      return
    }

    setOutputDir(ensured.path)
    setStatus('ok')
    await update({ outputDir: ensured.path })
  }, [pushToast, setOutputDir, setStatus, t, update])

  const resetToDefault = useCallback(async (): Promise<void> => {
    const bridge = getBridge()
    if (!bridge || systemDefault.length === 0) return
    const ensured = await bridge.ensureDir(systemDefault)
    setOutputDir(ensured.ok ? ensured.path : systemDefault)
    setStatus(ensured.ok ? 'ok' : 'invalid')
    await update({ outputDir: null })
  }, [setOutputDir, setStatus, systemDefault, update])

  const reveal = useCallback(async (): Promise<void> => {
    const bridge = getBridge()
    if (!bridge || outputDir.length === 0) return
    await bridge.revealInFolder(outputDir)
  }, [outputDir])

  return (
    <div className="flex shrink-0 items-center gap-2 rounded-lg border border-su-border bg-su-surface px-3 py-2">
      <span className="shrink-0 text-[12px] text-su-text-muted">{t('output.title')}</span>

      <span
        className={invalid ? 'min-w-0 flex-1 truncate text-[12px] text-su-error' : 'min-w-0 flex-1 truncate text-[12px] text-su-text'}
        title={outputDir}
      >
        {outputDir.length > 0 ? shortenPath(outputDir, 60) : t('common.loading')}
      </span>

      {invalid ? (
        <span className="shrink-0 text-[11px] text-su-error">{t('output.notWritable')}</span>
      ) : null}

      <div className="flex shrink-0 gap-1.5">
        <Button variant="ghost" size="sm" disabled={outputDir.length === 0} onClick={() => void reveal()}>
          {t('action.openOutput')}
        </Button>
        {isCustom ? (
          <Button variant="ghost" size="sm" onClick={() => void resetToDefault()}>
            {t('common.auto')}
          </Button>
        ) : null}
        <Button variant="secondary" size="sm" onClick={() => void choose()}>
          {t('output.change')}
        </Button>
      </div>
    </div>
  )
}
