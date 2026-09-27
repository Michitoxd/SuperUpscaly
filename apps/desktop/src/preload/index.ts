import { contextBridge, ipcRenderer, webUtils } from 'electron'
import {
  IPC,
  MAX_DROP_FILES,
  toRuntimePlatform,
  type AppInfo,
  type AppSettings,
  type ExpandArchivesResult,
  type ExpandFolderResult,
  type FileStatResult,
  type ModelDownloadEvent,
  type SidecarCapabilities,
  type SidecarEvent,
  type SidecarJob,
  type SidecarJobRequest,
  type SidecarModelsResult,
  type SidecarPipelinesResult,
  type SidecarStatus,
  type SuApi,
  type ValidatePathsResult,
} from '@superupscaly/shared'

/**
 * Obtiene la ruta real de cada `File` soltado sobre la ventana.
 *
 * Desde Electron 32 `File.path` ya no existe; el unico mecanismo valido es
 * `webUtils.getPathForFile`. Devuelve `''` en las posiciones que no se pueden
 * resolver (Wayland sin portal, por ejemplo) en lugar de lanzar: la UI necesita
 * saber cuantas fallaron para avisar al usuario.
 */
function getPathsForFiles(files: File[]): string[] {
  if (!Array.isArray(files)) return []
  const limited = files.slice(0, MAX_DROP_FILES)
  const out: string[] = []
  for (const file of limited) {
    try {
      const resolved = webUtils.getPathForFile(file)
      out.push(typeof resolved === 'string' ? resolved : '')
    } catch {
      out.push('')
    }
  }
  return out
}

const api: SuApi = {
  platform: toRuntimePlatform(process.platform),
  getPathsForFiles,
  validatePaths: (paths: string[]): Promise<ValidatePathsResult> =>
    ipcRenderer.invoke(IPC.pathsValidate, paths) as Promise<ValidatePathsResult>,
  pickImages: (): Promise<string[]> => ipcRenderer.invoke(IPC.filesPick) as Promise<string[]>,
  pickFolder: (title?: string): Promise<string | null> =>
    ipcRenderer.invoke(IPC.folderPick, title ?? null) as Promise<string | null>,
  expandFolder: (dir: string, recursive?: boolean): Promise<ExpandFolderResult> =>
    ipcRenderer.invoke(IPC.folderExpand, dir, recursive ?? true) as Promise<ExpandFolderResult>,
  expandArchives: (paths: string[]): Promise<ExpandArchivesResult> =>
    ipcRenderer.invoke(IPC.archivesExpand, paths) as Promise<ExpandArchivesResult>,
  getDefaultOutputDir: (): Promise<string> => ipcRenderer.invoke(IPC.outputDefault) as Promise<string>,
  ensureDir: (dir: string): Promise<{ ok: boolean; path: string }> =>
    ipcRenderer.invoke(IPC.dirEnsure, dir) as Promise<{ ok: boolean; path: string }>,
  statFiles: (paths: string[]): Promise<FileStatResult[]> =>
    ipcRenderer.invoke(IPC.filesStat, paths) as Promise<FileStatResult[]>,
  getSettings: (): Promise<AppSettings> => ipcRenderer.invoke(IPC.settingsGet) as Promise<AppSettings>,
  setSettings: (patch: Partial<AppSettings>): Promise<AppSettings> =>
    ipcRenderer.invoke(IPC.settingsSet, patch) as Promise<AppSettings>,
  getAppInfo: (): Promise<AppInfo> => ipcRenderer.invoke(IPC.appInfo) as Promise<AppInfo>,
  revealInFolder: (path: string): Promise<boolean> =>
    ipcRenderer.invoke(IPC.reveal, path) as Promise<boolean>,
  openLogsFolder: (): Promise<boolean> => ipcRenderer.invoke(IPC.openLogs) as Promise<boolean>,

  // --- Sidecar -------------------------------------------------------------

  sidecarStatus: (): Promise<SidecarStatus> =>
    ipcRenderer.invoke(IPC.sidecarStatus) as Promise<SidecarStatus>,
  sidecarCapabilities: (): Promise<SidecarCapabilities> =>
    ipcRenderer.invoke(IPC.sidecarCapabilities) as Promise<SidecarCapabilities>,
  sidecarModels: (): Promise<SidecarModelsResult> =>
    ipcRenderer.invoke(IPC.sidecarModels) as Promise<SidecarModelsResult>,
  sidecarPipelines: (): Promise<SidecarPipelinesResult> =>
    ipcRenderer.invoke(IPC.sidecarPipelines) as Promise<SidecarPipelinesResult>,
  createJob: (request: SidecarJobRequest): Promise<SidecarJob> =>
    ipcRenderer.invoke(IPC.sidecarCreateJob, request) as Promise<SidecarJob>,
  listJobs: (): Promise<SidecarJob[]> =>
    ipcRenderer.invoke(IPC.sidecarListJobs) as Promise<SidecarJob[]>,
  getJob: (id: string): Promise<SidecarJob> =>
    ipcRenderer.invoke(IPC.sidecarGetJob, id) as Promise<SidecarJob>,
  pauseJob: (id: string): Promise<SidecarJob> =>
    ipcRenderer.invoke(IPC.sidecarPauseJob, id) as Promise<SidecarJob>,
  resumeJob: (id: string): Promise<SidecarJob> =>
    ipcRenderer.invoke(IPC.sidecarResumeJob, id) as Promise<SidecarJob>,
  cancelJob: (id: string): Promise<SidecarJob> =>
    ipcRenderer.invoke(IPC.sidecarCancelJob, id) as Promise<SidecarJob>,
  restartSidecar: (): Promise<SidecarStatus> =>
    ipcRenderer.invoke(IPC.sidecarRestart) as Promise<SidecarStatus>,

  /**
   * Se suscribe a los eventos del sidecar.
   *
   * El primer argumento de `ipcRenderer.on` es el evento de Electron, que no es
   * serializable: se descarta antes de llamar al oyente. Sin eso, el puente
   * lanzaria un error al intentar clonarlo.
   */
  onSidecarEvent: (listener: (event: SidecarEvent) => void): (() => void) => {
    const wrapped = (_event: unknown, payload: unknown): void => {
      listener(payload as SidecarEvent)
    }
    ipcRenderer.on(IPC.sidecarEvent, wrapped)
    return () => {
      ipcRenderer.removeListener(IPC.sidecarEvent, wrapped)
    }
  },

  onSidecarStatus: (listener: (status: SidecarStatus) => void): (() => void) => {
    const wrapped = (_event: unknown, payload: unknown): void => {
      listener(payload as SidecarStatus)
    }
    ipcRenderer.on(IPC.sidecarStatusChanged, wrapped)
    return () => {
      ipcRenderer.removeListener(IPC.sidecarStatusChanged, wrapped)
    }
  },

  // --- Descarga de modelos --------------------------------------------------

  downloadModel: (modelId: string): Promise<ModelDownloadEvent> =>
    ipcRenderer.invoke(IPC.modelDownload, modelId) as Promise<ModelDownloadEvent>,

  cancelModelDownload: (modelId: string): Promise<boolean> =>
    ipcRenderer.invoke(IPC.modelDownloadCancel, modelId) as Promise<boolean>,

  onModelDownload: (listener: (event: ModelDownloadEvent) => void): (() => void) => {
    const wrapped = (_event: unknown, payload: unknown): void => {
      listener(payload as ModelDownloadEvent)
    }
    ipcRenderer.on(IPC.modelDownloadEvent, wrapped)
    return () => {
      ipcRenderer.removeListener(IPC.modelDownloadEvent, wrapped)
    }
  },
}

contextBridge.exposeInMainWorld('su', api)
