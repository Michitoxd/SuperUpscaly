import type { SuApi } from '@superupscaly/shared'

declare global {
  interface Window {
    /**
     * Puente expuesto por el preload de Electron.
     *
     * Es opcional a proposito: durante el export estatico de Next y si alguien
     * abre la pagina en un navegador normal, `window.su` no existe y la UI debe
     * seguir renderizando en lugar de reventar.
     */
    su?: SuApi
  }
}

export {}
