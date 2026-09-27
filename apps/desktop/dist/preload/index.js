"use strict";

// apps/desktop/src/preload/index.ts
var import_electron = require("electron");

// packages/shared/src/types.ts
function toRuntimePlatform(value) {
  return value === "win32" || value === "darwin" || value === "linux" ? value : "unknown";
}
var IMAGE_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "webp",
  "bmp",
  "tif",
  "tiff",
  "avif"
];
var ARCHIVE_EXTENSIONS = ["zip", "cbz"];
var ALL_INPUT_EXTENSIONS = [...IMAGE_EXTENSIONS, ...ARCHIVE_EXTENSIONS];
var MAX_DROP_FILES = 5e3;

// packages/shared/src/media.ts
var APP_SCHEME = "app";
var APP_ORIGIN = `${APP_SCHEME}://superupscaly`;

// packages/shared/src/api.ts
var IPC = {
  pathsValidate: "su:paths:validate",
  filesPick: "su:files:pick",
  folderPick: "su:folder:pick",
  folderExpand: "su:folder:expand",
  archivesExpand: "su:archives:expand",
  outputDefault: "su:output:default",
  dirEnsure: "su:dir:ensure",
  filesStat: "su:files:stat",
  settingsGet: "su:settings:get",
  settingsSet: "su:settings:set",
  appInfo: "su:app:info",
  reveal: "su:app:reveal",
  openLogs: "su:app:open-logs",
  // Sidecar: peticiones
  sidecarStatus: "su:sidecar:status",
  sidecarCapabilities: "su:sidecar:capabilities",
  sidecarModels: "su:sidecar:models",
  sidecarPipelines: "su:sidecar:pipelines",
  sidecarCreateJob: "su:sidecar:job:create",
  sidecarListJobs: "su:sidecar:job:list",
  sidecarGetJob: "su:sidecar:job:get",
  sidecarPauseJob: "su:sidecar:job:pause",
  sidecarResumeJob: "su:sidecar:job:resume",
  sidecarCancelJob: "su:sidecar:job:cancel",
  sidecarRestart: "su:sidecar:restart",
  // Descarga de modelos
  modelDownload: "su:models:download",
  modelDownloadCancel: "su:models:download-cancel",
  // Sidecar: notificaciones del proceso principal al renderer
  sidecarEvent: "su:sidecar:event",
  sidecarStatusChanged: "su:sidecar:status-changed",
  modelDownloadEvent: "su:models:download-event"
};

// packages/shared/src/theme.ts
var palette = {
  bgBase: "#1E1B2E",
  bgSurface: "#2D2640",
  bgSurface2: "#382F52",
  accent: "#8B5CF6",
  accentHover: "#A78BFA",
  accentMuted: "#6D28D9",
  textPrimary: "#F3F4F6",
  textSecondary: "#C4B5FD",
  border: "#4C1D95",
  success: "#34D399",
  warning: "#FBBF24",
  error: "#F87171"
};
var statusColor = {
  pending: palette.textSecondary,
  running: palette.accentHover,
  done: palette.success,
  degraded: palette.warning,
  failed: palette.error,
  skipped: palette.textSecondary
};

// packages/shared/src/i18n/es.ts
var es = {
  app: {
    name: "SuperUpscaly",
    tagline: "Upscaling local con IA"
  },
  common: {
    close: "Cerrar",
    cancel: "Cancelar",
    retry: "Reintentar",
    auto: "Automatico",
    on: "Activado",
    off: "Desactivado",
    seconds: "s",
    minutes: "min",
    hours: "h",
    clear: "Vaciar lista",
    remove: "Quitar",
    loading: "Cargando\u2026"
  },
  mode: {
    title: "Modo",
    photo: "Fotos",
    illustration: "Dibujo / Anime",
    recommended: "Recomendado",
    hint: "Determina la cadena de modelos que se aplicara."
  },
  scale: {
    title: "Escala"
  },
  models: {
    title: "Modelos",
    subtitle: "Se descargan en el primer uso desde su origen original.",
    open: "Modelos",
    empty: "El motor aun no ha publicado su catalogo.",
    missingCount: "{count} sin descargar",
    download: "Descargar",
    redownload: "Volver a descargar",
    downloading: "Descargando",
    openFolder: "Abrir carpeta",
    sizeUnknown: "tamano desconocido",
    noDownload: "Sin origen declarado: instala este modelo a mano en la carpeta.",
    state: {
      installed: "Instalado",
      missing: "Sin descargar",
      hashMismatch: "Archivo incorrecto",
      unverified: "Sin verificar"
    },
    kind: {
      photo: "Foto",
      illustration: "Dibujo",
      denoise: "Ruido",
      face: "Rostros",
      detector: "Deteccion",
      classifier: "Clasificacion"
    }
  },
  drop: {
    title: "Arrastra y suelta tus imagenes aqui",
    hint: "PNG \xB7 JPG \xB7 WEBP \xB7 BMP \xB7 TIFF \xB7 AVIF \xB7 ZIP / CBZ",
    button: "Seleccionar imagen(es)",
    dragging: "Suelta para anadir",
    rejected: "No se pudieron leer {count} archivo(s).",
    rejectedAction: "Seleccionalos con el boton de archivos.",
    limitExceeded: "Se ignoraron {count} archivo(s): el maximo por tanda es {max}.",
    archiveFailed: "No se pudo abrir {count} archivo(s) comprimido(s).",
    emptyFolder: "La carpeta no contiene imagenes compatibles."
  },
  output: {
    title: "Carpeta de salida",
    change: "Cambiar",
    pickTitle: "Elige la carpeta de salida",
    notWritable: "No se puede escribir en esa carpeta."
  },
  action: {
    upscaly: "Upscaly",
    cancel: "Cancelar",
    pause: "Pausar",
    resume: "Reanudar",
    openOutput: "Abrir carpeta de salida",
    exportLogs: "Exportar diagnostico"
  },
  advanced: {
    title: "Avanzado",
    collapsedHint: "Tile, dispositivo, cadena de modelos, restauracion facial",
    noBackend: "El motor de escalado no esta listo: no se enviara ningun trabajo.",
    tile: {
      label: "Tamano de tile",
      hint: "En Automatico se calcula a partir de la VRAM libre. Forzarlo solo si sabes lo que haces."
    },
    device: {
      label: "Dispositivo",
      gpu: "GPU",
      cpu: "CPU",
      hint: "En Automatico se usa la mejor GPU disponible y se cae a CPU si no hay ninguna."
    },
    models: {
      label: "Cadena de modelos",
      manual: "Manual",
      upscale: "Modelo base",
      hint: "En Automatico la app elige el modelo segun el modo y el analisis de la imagen."
    },
    face: {
      label: "Restauracion facial",
      low: "Suave",
      medium: "Media",
      high: "Fuerte",
      hint: "Solo se aplica sobre los rostros detectados.",
      photoOnly: "Solo disponible en modo Fotos."
    },
    denoise: {
      label: "Reduccion de ruido",
      hint: "Se ejecuta antes del escalado. Automatico lo activa si la imagen esta degradada."
    },
    sharpen: {
      label: "Enfoque final",
      hint: "Se inhibe automaticamente en imagenes con artefactos de compresion."
    },
    concurrency: {
      label: "Imagenes simultaneas",
      hint: "1 por GPU es el valor optimo: dos sesiones no aceleran y duplican la VRAM."
    },
    unload: {
      label: "Liberar modelo entre imagenes",
      hint: "Reduce el pico de VRAM a costa de recargar el modelo cada vez."
    },
    format: {
      label: "Formato de salida",
      quality: "Calidad",
      suffix: "Sufijo del archivo",
      preserveMetadata: "Conservar metadatos"
    }
  },
  queue: {
    title: "Cola",
    empty: "Sin imagenes en la cola",
    emptyHint: "Arrastra archivos o usa el boton de seleccion.",
    colName: "Archivo",
    colSize: "Tamano",
    colStatus: "Estado",
    colProgress: "Progreso",
    colTime: "Tiempo",
    clear: "Vaciar lista"
  },
  status: {
    pending: "En espera",
    running: "Procesando",
    done: "Completado",
    degraded: "Degradado",
    failed: "Fallido",
    skipped: "Omitido"
  },
  progress: {
    title: "Procesando",
    global: "Progreso global",
    eta: "Tiempo restante",
    preparingModel: "Preparando: descargando el modelo",
    stage: {
      decode: "Decodificando",
      analyze: "Analizando",
      denoise: "Reduciendo ruido",
      upscale: "Escalando",
      face: "Restaurando rostros",
      sharpen: "Enfocando",
      encode: "Guardando"
    }
  },
  summary: {
    title: "Resumen",
    success: "Completadas",
    failed: "Fallidas",
    degraded: "Degradadas",
    totalTime: "Tiempo total",
    avgTime: "Media por imagen",
    notes: "Que se hizo distinto",
    notesHint: "Una etapa puede omitirse si su condicion no se cumple, o usar el modelo de reserva si el suyo no esta instalado. Aqui aparece lo que paso en cada imagen."
  },
  compare: {
    title: "Comparar antes y despues",
    open: "Comparar",
    original: "Original",
    upscaled: "Escalado",
    /** Nombre accesible de la linea que se arrastra. */
    divider: "Linea de comparacion",
    hint: "Arrastra la linea sobre la imagen. Con el teclado, las flechas la mueven.",
    dimensions: "{width} x {height} px",
    failedOriginal: "No se puede previsualizar el original: este formato (TIFF, BMP) no se dibuja en la ventana.",
    failedUpscaled: "No se puede previsualizar el resultado: es demasiado grande para mostrarlo aqui.",
    failedHint: "El archivo si esta guardado: abrelo desde la carpeta de salida.",
    openFolder: "Abrir carpeta"
  },
  error: {
    title: "Se produjo un error",
    details: "Detalle tecnico",
    copy: "Copiar detalle",
    openLogs: "Abrir carpeta de logs",
    actionLabel: "Que puedes hacer"
  },
  settings: {
    language: "Idioma",
    spanish: "Espanol",
    english: "Ingles"
  },
  /** La pagina se abrio fuera de Electron: no hay puente y no se puede escalar. */
  noBridge: {
    body: "Esta pagina se esta mostrando fuera de la aplicacion, asi que no puede hablar con el motor. Abrela con `npm start`."
  },
  sidecar: {
    title: "Motor de escalado",
    restart: "Reiniciar",
    restarting: "Reiniciando el motor\u2026",
    missingBinary: "No se encontro el ejecutable del sidecar. Compilalo con: cargo build --release -p su-cli",
    notReady: "El motor de escalado no esta disponible todavia.",
    jobRejected: "El motor rechazo el trabajo.",
    device: "Dispositivo",
    cores: "Nucleos",
    state: {
      stopped: "Detenido",
      starting: "Arrancando",
      ready: "Listo",
      restarting: "Reiniciando",
      failed: "Con error",
      unavailable: "No disponible"
    }
  },
  errors: {
    "SU-E001": {
      message: "No se encontro ninguna imagen compatible.",
      action: "Comprueba que los archivos son imagenes o archivos ZIP/CBZ validos."
    },
    "SU-E100": {
      message: "No se pudo decodificar el archivo.",
      action: "Prueba a abrirlo en otro programa para descartar que este danado."
    },
    "SU-E101": {
      message: "El formato de la imagen no esta soportado.",
      action: "Convierte la imagen a PNG, JPG, WEBP, BMP o TIFF."
    },
    "SU-E102": {
      message: "El archivo esta corrupto o incompleto.",
      action: "Vuelve a copiarlo desde el origen."
    },
    "SU-E110": {
      message: "Falta el modelo necesario para este modo.",
      action: "Abre el gestor de modelos y descargalo."
    },
    "SU-E111": {
      message: "El modelo descargado no coincide con el hash esperado.",
      action: "Elimina el modelo y vuelve a descargarlo."
    },
    "SU-E112": {
      message: "No se pudo descargar el modelo desde su origen.",
      action: "Comprueba la conexion y reintenta. Si usas un proxy, configuralo en los ajustes."
    },
    "SU-E120": {
      message: "La aceleracion por hardware no esta disponible; se usa un modo mas lento.",
      action: "Actualiza el driver de la grafica si quieres recuperar el rendimiento completo."
    },
    "SU-E121": {
      message: "No se pudo compilar el motor de TensorRT.",
      action: "Se usara CUDA en su lugar. Actualizar el driver suele resolverlo."
    },
    "SU-E130": {
      message: "Se agoto la memoria de la grafica.",
      action: "Se reintento con tiles mas pequenos. Reduce el tile o cierra otras aplicaciones con GPU."
    },
    "SU-E131": {
      message: "Se perdio la conexion con el dispositivo grafico.",
      action: "Guarda tu trabajo y reinicia la aplicacion."
    },
    "SU-E140": {
      message: "Fallo el procesado de un fragmento de la imagen.",
      action: "Se reintento con una configuracion mas conservadora."
    },
    "SU-E141": {
      message: "El resultado no supero la validacion y no se guardo.",
      action: "Prueba con otro modelo o reduce el factor de escala."
    },
    "SU-E142": {
      message: "A esa escala la imagen resultante superaria el limite de megapixeles.",
      action: "Baja el factor de escala o sube el limite de megapixeles en los ajustes avanzados."
    },
    "SU-E143": {
      message: "El resultado no alcanzo la escala que promete el pipeline.",
      action: "Es un fallo interno del pipeline, no de tu imagen. Informa de este codigo en el repositorio."
    },
    "SU-E150": {
      message: "No se pudo escribir el archivo de salida.",
      action: "Comprueba los permisos y el espacio libre en disco."
    },
    "SU-E160": {
      message: "Operacion cancelada.",
      action: "Puedes reanudarla cuando quieras."
    },
    "SU-E161": {
      message: "No se pudo obtener la ruta del archivo arrastrado.",
      action: "Usa el boton de seleccionar archivos en su lugar."
    },
    "SU-E900": {
      message: "Error interno inesperado.",
      action: "Exporta el diagnostico y adjuntalo al informe de error."
    }
  }
};

// packages/shared/src/i18n/index.ts
var _todo_codigo_de_error_tiene_texto = es.errors;

// apps/desktop/src/preload/index.ts
function getPathsForFiles(files) {
  if (!Array.isArray(files)) return [];
  const limited = files.slice(0, MAX_DROP_FILES);
  const out = [];
  for (const file of limited) {
    try {
      const resolved = import_electron.webUtils.getPathForFile(file);
      out.push(typeof resolved === "string" ? resolved : "");
    } catch {
      out.push("");
    }
  }
  return out;
}
var api = {
  platform: toRuntimePlatform(process.platform),
  getPathsForFiles,
  validatePaths: (paths) => import_electron.ipcRenderer.invoke(IPC.pathsValidate, paths),
  pickImages: () => import_electron.ipcRenderer.invoke(IPC.filesPick),
  pickFolder: (title) => import_electron.ipcRenderer.invoke(IPC.folderPick, title ?? null),
  expandFolder: (dir, recursive) => import_electron.ipcRenderer.invoke(IPC.folderExpand, dir, recursive ?? true),
  expandArchives: (paths) => import_electron.ipcRenderer.invoke(IPC.archivesExpand, paths),
  getDefaultOutputDir: () => import_electron.ipcRenderer.invoke(IPC.outputDefault),
  ensureDir: (dir) => import_electron.ipcRenderer.invoke(IPC.dirEnsure, dir),
  statFiles: (paths) => import_electron.ipcRenderer.invoke(IPC.filesStat, paths),
  getSettings: () => import_electron.ipcRenderer.invoke(IPC.settingsGet),
  setSettings: (patch) => import_electron.ipcRenderer.invoke(IPC.settingsSet, patch),
  getAppInfo: () => import_electron.ipcRenderer.invoke(IPC.appInfo),
  revealInFolder: (path) => import_electron.ipcRenderer.invoke(IPC.reveal, path),
  openLogsFolder: () => import_electron.ipcRenderer.invoke(IPC.openLogs),
  // --- Sidecar -------------------------------------------------------------
  sidecarStatus: () => import_electron.ipcRenderer.invoke(IPC.sidecarStatus),
  sidecarCapabilities: () => import_electron.ipcRenderer.invoke(IPC.sidecarCapabilities),
  sidecarModels: () => import_electron.ipcRenderer.invoke(IPC.sidecarModels),
  sidecarPipelines: () => import_electron.ipcRenderer.invoke(IPC.sidecarPipelines),
  createJob: (request) => import_electron.ipcRenderer.invoke(IPC.sidecarCreateJob, request),
  listJobs: () => import_electron.ipcRenderer.invoke(IPC.sidecarListJobs),
  getJob: (id) => import_electron.ipcRenderer.invoke(IPC.sidecarGetJob, id),
  pauseJob: (id) => import_electron.ipcRenderer.invoke(IPC.sidecarPauseJob, id),
  resumeJob: (id) => import_electron.ipcRenderer.invoke(IPC.sidecarResumeJob, id),
  cancelJob: (id) => import_electron.ipcRenderer.invoke(IPC.sidecarCancelJob, id),
  restartSidecar: () => import_electron.ipcRenderer.invoke(IPC.sidecarRestart),
  /**
   * Se suscribe a los eventos del sidecar.
   *
   * El primer argumento de `ipcRenderer.on` es el evento de Electron, que no es
   * serializable: se descarta antes de llamar al oyente. Sin eso, el puente
   * lanzaria un error al intentar clonarlo.
   */
  onSidecarEvent: (listener) => {
    const wrapped = (_event, payload) => {
      listener(payload);
    };
    import_electron.ipcRenderer.on(IPC.sidecarEvent, wrapped);
    return () => {
      import_electron.ipcRenderer.removeListener(IPC.sidecarEvent, wrapped);
    };
  },
  onSidecarStatus: (listener) => {
    const wrapped = (_event, payload) => {
      listener(payload);
    };
    import_electron.ipcRenderer.on(IPC.sidecarStatusChanged, wrapped);
    return () => {
      import_electron.ipcRenderer.removeListener(IPC.sidecarStatusChanged, wrapped);
    };
  },
  // --- Descarga de modelos --------------------------------------------------
  downloadModel: (modelId) => import_electron.ipcRenderer.invoke(IPC.modelDownload, modelId),
  cancelModelDownload: (modelId) => import_electron.ipcRenderer.invoke(IPC.modelDownloadCancel, modelId),
  onModelDownload: (listener) => {
    const wrapped = (_event, payload) => {
      listener(payload);
    };
    import_electron.ipcRenderer.on(IPC.modelDownloadEvent, wrapped);
    return () => {
      import_electron.ipcRenderer.removeListener(IPC.modelDownloadEvent, wrapped);
    };
  }
};
import_electron.contextBridge.exposeInMainWorld("su", api);
//# sourceMappingURL=index.js.map
