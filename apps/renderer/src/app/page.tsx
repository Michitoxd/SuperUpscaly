'use client'

import { useEffect } from 'react'
import { useAtomValue, useSetAtom } from 'jotai'
import { ActionBar } from '@/components/ActionBar'
import { CompareView } from '@/components/CompareView'
import { DropZone } from '@/components/DropZone'
import { ModelsDialog } from '@/components/ModelsDialog'
import { OutputFolderPicker } from '@/components/OutputFolderPicker'
import { Sidebar } from '@/components/Sidebar'
import { SummaryDialog } from '@/components/SummaryDialog'
import { Toaster } from '@/components/Toaster'
import { bootstrapAtom } from '@/state/atoms/app'
import { localeAtom } from '@/state/atoms/settings'
import { useSidecarEvents } from '@/state/useSidecarEvents'

export default function HomePage() {
  const bootstrap = useSetAtom(bootstrapAtom)
  const locale = useAtomValue(localeAtom)

  // El flujo de eventos del sidecar se consume una sola vez, aqui: si viviera en
  // un componente que se monta y desmonta, cada cambio de pantalla perderia
  // eventos por el hueco entre la baja y la nueva suscripcion.
  useSidecarEvents()

  useEffect(() => {
    void bootstrap()
  }, [bootstrap])

  // El export estatico fija `lang="es"` en el HTML; se corrige en cliente para
  // que los lectores de pantalla pronuncien bien cuando el idioma es ingles.
  useEffect(() => {
    document.documentElement.lang = locale
  }, [locale])

  return (
    <div className="relative flex h-screen w-screen overflow-hidden bg-su-base">
      <Sidebar />

      <main className="flex min-w-0 flex-1 flex-col gap-3 p-3">
        <DropZone />
        <OutputFolderPicker />
        <ActionBar />
      </main>

      <CompareView />
      <SummaryDialog />
      <ModelsDialog />
      <Toaster />
    </div>
  )
}
