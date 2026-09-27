"use strict";

// apps/desktop/src/main/index.ts
var import_electron7 = require("electron");
var import_node_path15 = require("node:path");

// apps/desktop/src/main/ipc/index.ts
var import_electron4 = require("electron");
var import_promises3 = require("node:fs/promises");
var import_node_fs4 = require("node:fs");
var import_node_path6 = require("node:path");

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
var MAX_PATH_LENGTH = 4096;
var MAX_DROP_FILES = 5e3;
var MAX_STAT_FILES = 2e4;

// packages/shared/src/media.ts
var APP_SCHEME = "app";
var APP_ORIGIN = `${APP_SCHEME}://superupscaly`;
var MEDIA_ROUTE = "/media";
var MEDIA_PARAM = "p";
function parseMediaUrl(rawUrl) {
  let url;
  try {
    url = new URL(rawUrl);
  } catch {
    return null;
  }
  if (url.protocol !== `${APP_SCHEME}:`) return null;
  if (url.host !== "superupscaly") return null;
  if (url.pathname !== MEDIA_ROUTE) return null;
  const requested = url.searchParams.get(MEDIA_PARAM);
  return requested === null || requested.length === 0 ? null : requested;
}
function isMediaPath(pathname) {
  return pathname === MEDIA_ROUTE;
}

// packages/shared/src/sidecar.ts
var SIDECAR_PROTOCOL_VERSION = 1;
var MAX_SIDECAR_RESTARTS = 5;

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

// packages/shared/src/guards.ts
function isPlainObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function isString(value) {
  return typeof value === "string";
}
function isBoolean(value) {
  return typeof value === "boolean";
}
function isFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value);
}
function isIntegerInRange(value, min, max) {
  return isFiniteNumber(value) && Number.isInteger(value) && value >= min && value <= max;
}
function isOneOf(value, allowed2) {
  return allowed2.includes(value);
}
function safePathRejection(value) {
  if (typeof value !== "string") return "empty";
  if (value.length === 0) return "empty";
  if (value.includes("\0")) return "empty";
  if (value.length > MAX_PATH_LENGTH) return "too-long";
  return null;
}
function isSafePathString(value) {
  return safePathRejection(value) === null;
}
function toSafePathArray(value, maxItems) {
  if (!Array.isArray(value)) return [];
  const out = [];
  for (const entry of value) {
    if (out.length >= maxItems) break;
    if (isSafePathString(entry)) out.push(entry);
  }
  return out;
}
var MODES = ["photo", "illustration"];
var SCALES = [2, 4, 8];
var FORMATS = ["png", "jpg", "webp"];
var LOCALES = ["es", "en"];
var DEVICES = ["auto", "cpu", "gpu"];
var TILES = ["auto", 256, 384, 512, 768, 1024];
var CHAIN_MODES = ["auto", "manual"];
var DENOISE = ["off", "auto", "on"];
var FACE = ["off", "auto", "low", "medium", "high"];
function sanitizeSettingsPatch(value) {
  if (!isPlainObject(value)) return {};
  const out = {};
  if (isOneOf(value["locale"], LOCALES)) out.locale = value["locale"];
  if (value["outputDir"] === null) out.outputDir = null;
  else if (isSafePathString(value["outputDir"])) out.outputDir = value["outputDir"];
  if (isOneOf(value["mode"], MODES)) out.mode = value["mode"];
  if (isOneOf(value["scale"], SCALES)) out.scale = value["scale"];
  if (isOneOf(value["outputFormat"], FORMATS)) out.outputFormat = value["outputFormat"];
  if (isIntegerInRange(value["outputQuality"], 1, 100)) out.outputQuality = value["outputQuality"];
  if (isString(value["suffix"]) && value["suffix"].length <= 32) {
    out.suffix = value["suffix"].replace(/[\\/:*?"<>|\u0000]/g, "");
  }
  if (isBoolean(value["preserveMetadata"])) out.preserveMetadata = value["preserveMetadata"];
  if (isBoolean(value["advancedOpen"])) out.advancedOpen = value["advancedOpen"];
  if (isOneOf(value["tileSize"], TILES)) out.tileSize = value["tileSize"];
  if (isOneOf(value["device"], DEVICES)) out.device = value["device"];
  if (isIntegerInRange(value["concurrency"], 1, 8)) out.concurrency = value["concurrency"];
  if (isBoolean(value["unloadBetweenImages"])) out.unloadBetweenImages = value["unloadBetweenImages"];
  if (isOneOf(value["modelChainMode"], CHAIN_MODES)) out.modelChainMode = value["modelChainMode"];
  if (isString(value["upscaleModel"]) && value["upscaleModel"].length <= 64) {
    out.upscaleModel = value["upscaleModel"];
  }
  if (isOneOf(value["faceRestore"], FACE)) out.faceRestore = value["faceRestore"];
  if (isOneOf(value["denoise"], DENOISE)) out.denoise = value["denoise"];
  if (isBoolean(value["sharpen"])) out.sharpen = value["sharpen"];
  return out;
}
var DEFAULT_SETTINGS = {
  locale: "es",
  outputDir: null,
  mode: "photo",
  scale: 4,
  outputFormat: "png",
  outputQuality: 95,
  suffix: "_upscaled",
  preserveMetadata: true,
  advancedOpen: false,
  tileSize: "auto",
  device: "auto",
  concurrency: 1,
  unloadBetweenImages: false,
  modelChainMode: "auto",
  upscaleModel: "",
  faceRestore: "auto",
  denoise: "auto",
  sharpen: false
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

// apps/desktop/src/main/files/pathguard.ts
var import_node_fs2 = require("node:fs");
var import_promises = require("node:fs/promises");
var import_node_path2 = require("node:path");

// apps/desktop/src/main/logging/logger.ts
var import_electron = require("electron");
var import_node_fs = require("node:fs");
var import_node_os = require("node:os");
var import_node_path = require("node:path");
var LEVEL_ORDER = { debug: 10, info: 20, warn: 30, error: 40 };
var stream = null;
var logDirectory = "";
var minLevel = "info";
var home = (0, import_node_os.homedir)();
function mask(value) {
  if (!home) return value;
  return value.split(home).join("<home>");
}
function initLogger() {
  logDirectory = (0, import_node_path.join)(import_electron.app.getPath("userData"), "logs");
  (0, import_node_fs.mkdirSync)(logDirectory, { recursive: true });
  const stamp = (/* @__PURE__ */ new Date()).toISOString().slice(0, 10);
  stream = (0, import_node_fs.createWriteStream)((0, import_node_path.join)(logDirectory, `superupscaly-${stamp}.log`), { flags: "a" });
  minLevel = import_electron.app.isPackaged ? "info" : "debug";
  log("info", "logger.ready", { dir: logDirectory, level: minLevel });
  return logDirectory;
}
function getLogDirectory() {
  return logDirectory;
}
function log(level, message, fields) {
  if (LEVEL_ORDER[level] < LEVEL_ORDER[minLevel]) return;
  const entry = {
    ts: (/* @__PURE__ */ new Date()).toISOString(),
    level,
    msg: message,
    ...fields
  };
  try {
    stream?.write(`${JSON.stringify(entry, (_key, value) => typeof value === "string" ? mask(value) : value)}
`);
  } catch {
  }
  if (!import_electron.app.isPackaged) {
    const printer = level === "error" ? console.error : level === "warn" ? console.warn : console.log;
    printer(`[${level}] ${message}`, fields ?? "");
  }
}
var logger = {
  debug: (message, fields) => log("debug", message, fields),
  info: (message, fields) => log("info", message, fields),
  warn: (message, fields) => log("warn", message, fields),
  error: (message, fields) => log("error", message, fields)
};

// apps/desktop/src/main/media/registry.ts
var allowed = /* @__PURE__ */ new Set();
function registerMediaPath(absolutePath) {
  if (!isSafePathString(absolutePath)) return false;
  allowed.add(absolutePath);
  return true;
}
function isMediaAllowed(absolutePath) {
  return allowed.has(absolutePath);
}

// apps/desktop/src/main/files/pathguard.ts
var MAX_EXPAND_FILES = 2e4;
var MAX_EXPAND_DEPTH = 12;
function hasSupportedExtension(filePath) {
  const ext = (0, import_node_path2.extname)(filePath).slice(1).toLowerCase();
  return ALL_INPUT_EXTENSIONS.includes(ext);
}
async function expandFolder(dir, recursive = true) {
  const found = [];
  let scanned = 0;
  let truncated = false;
  const queue = [{ dir, depth: 0 }];
  while (queue.length > 0) {
    const current = queue.shift();
    if (!current) break;
    let entries;
    try {
      entries = await (0, import_promises.readdir)(current.dir, { withFileTypes: true });
    } catch (error) {
      logger.warn("folder.read-failed", { code: error.code ?? "unknown" });
      continue;
    }
    for (const entry of entries) {
      if (found.length >= MAX_EXPAND_FILES) {
        truncated = true;
        break;
      }
      if (entry.name.startsWith(".")) continue;
      const full = (0, import_node_path2.join)(current.dir, entry.name);
      if (entry.isDirectory()) {
        if (recursive && current.depth < MAX_EXPAND_DEPTH) {
          queue.push({ dir: full, depth: current.depth + 1 });
        }
        continue;
      }
      if (!entry.isFile()) continue;
      scanned += 1;
      if (hasSupportedExtension(full)) found.push(full);
    }
    if (truncated) break;
  }
  found.sort((a, b) => a.localeCompare(b, void 0, { numeric: true, sensitivity: "base" }));
  if (truncated) logger.warn("folder.truncated", { scanned, kept: found.length, limit: MAX_EXPAND_FILES });
  return { paths: found, scanned, truncated };
}
async function validatePaths(input) {
  const accepted = [];
  const rejected = [];
  const seen = /* @__PURE__ */ new Set();
  const accept = (candidate) => {
    if (seen.has(candidate)) return;
    seen.add(candidate);
    accepted.push(candidate);
    registerMediaPath(candidate);
  };
  for (const raw of input) {
    const rejection = safePathRejection(raw);
    if (rejection) {
      rejected.push({ path: typeof raw === "string" ? raw.slice(0, 200) : "", reason: rejection });
      continue;
    }
    if (!(0, import_node_path2.isAbsolute)(raw)) {
      rejected.push({ path: raw, reason: "not-absolute" });
      continue;
    }
    let resolved;
    try {
      resolved = await (0, import_promises.realpath)(raw);
    } catch {
      rejected.push({ path: raw, reason: "not-found" });
      continue;
    }
    if (resolved.length > MAX_PATH_LENGTH) {
      rejected.push({ path: resolved.slice(0, 200), reason: "too-long" });
      continue;
    }
    try {
      await (0, import_promises.access)(resolved, import_node_fs2.constants.R_OK);
    } catch {
      rejected.push({ path: resolved, reason: "not-readable" });
      continue;
    }
    let info;
    try {
      info = await (0, import_promises.stat)(resolved);
    } catch {
      rejected.push({ path: resolved, reason: "not-found" });
      continue;
    }
    if (info.isDirectory()) {
      const expanded = await expandFolder(resolved, true);
      if (expanded.paths.length === 0) {
        rejected.push({ path: resolved, reason: "empty-folder" });
        continue;
      }
      for (const candidate of expanded.paths) accept(candidate);
      continue;
    }
    if (!info.isFile()) {
      rejected.push({ path: resolved, reason: "not-a-file" });
      continue;
    }
    if (!hasSupportedExtension(resolved)) {
      rejected.push({ path: resolved, reason: "unsupported-extension" });
      continue;
    }
    accept(resolved);
  }
  logger.debug("paths.validated", { input: input.length, accepted: accepted.length, rejected: rejected.length });
  return { accepted, rejected };
}
async function statFiles(paths) {
  const results = [];
  for (const candidate of paths) {
    if (safePathRejection(candidate) !== null) continue;
    try {
      const info = await (0, import_promises.stat)(candidate);
      results.push({
        path: candidate,
        sizeBytes: info.size,
        modifiedAt: Math.round(info.mtimeMs)
      });
    } catch {
    }
  }
  return results;
}

// apps/desktop/src/main/archives/service.ts
var import_node_crypto = require("node:crypto");
var import_node_os2 = require("node:os");
var import_node_path4 = require("node:path");

// apps/desktop/src/main/archives/reader.ts
var import_promises2 = require("node:fs/promises");
var import_node_path3 = require("node:path");
var import_node_zlib = require("node:zlib");
var EOCD = 101010256;
var CENTRAL = 33639248;
var LOCAL = 67324752;
var EOCD_MIN = 22;
var MAX_COMENTARIO = 65535;
var ALMACENADO = 0;
var DEFLATE = 8;
var FLAG_CIFRADO = 1;
var ZIP64 = 4294967295;
var ZIP64_ENTRADAS = 65535;
function fallo(message, errorCode = "SU-E102") {
  return { status: "failed", errorCode, message };
}
function mensajeDe(error) {
  return error instanceof Error ? error.message : String(error);
}
function localizarEocd(buf) {
  if (buf.length < EOCD_MIN) {
    return fallo("el archivo es demasiado pequeno para ser un ZIP");
  }
  const desde = Math.max(0, buf.length - EOCD_MIN - MAX_COMENTARIO);
  for (let i = buf.length - EOCD_MIN; i >= desde; i--) {
    if (buf.readUInt32LE(i) === EOCD) return i;
  }
  return fallo("no se encontro el final del directorio central: el archivo no es un ZIP o esta truncado");
}
function leerDirectorioCentral(buf, eocd) {
  const total = buf.readUInt16LE(eocd + 10);
  const offset = buf.readUInt32LE(eocd + 16);
  if (total === ZIP64_ENTRADAS || offset === ZIP64) {
    return fallo("el archivo usa ZIP64, que no se soporta");
  }
  const entradas = [];
  let cursor = offset;
  for (let i = 0; i < total; i++) {
    if (cursor + 46 > buf.length) {
      return fallo("el directorio central esta truncado");
    }
    if (buf.readUInt32LE(cursor) !== CENTRAL) {
      return fallo(`la entrada ${i + 1} del directorio central tiene una firma invalida`);
    }
    const flags = buf.readUInt16LE(cursor + 8);
    const method = buf.readUInt16LE(cursor + 10);
    const crc = buf.readUInt32LE(cursor + 16);
    const compressedSize = buf.readUInt32LE(cursor + 20);
    const uncompressedSize = buf.readUInt32LE(cursor + 24);
    const nameLen = buf.readUInt16LE(cursor + 28);
    const extraLen = buf.readUInt16LE(cursor + 30);
    const commentLen = buf.readUInt16LE(cursor + 32);
    const localOffset = buf.readUInt32LE(cursor + 42);
    const inicioNombre = cursor + 46;
    if (inicioNombre + nameLen > buf.length) {
      return fallo(`el nombre de la entrada ${i + 1} esta truncado`);
    }
    entradas.push({
      name: buf.toString("utf8", inicioNombre, inicioNombre + nameLen),
      method,
      flags,
      crc,
      compressedSize,
      uncompressedSize,
      localOffset
    });
    cursor = inicioNombre + nameLen + extraLen + commentLen;
  }
  return entradas;
}
function extraerEntrada(buf, entrada) {
  if ((entrada.flags & FLAG_CIFRADO) !== 0) {
    return fallo(`"${entrada.name}" va cifrada: no se soportan contrasenas`);
  }
  if (entrada.compressedSize === ZIP64 || entrada.localOffset === ZIP64) {
    return fallo(`"${entrada.name}" usa ZIP64, que no se soporta`);
  }
  if (entrada.method !== ALMACENADO && entrada.method !== DEFLATE) {
    return fallo(
      `"${entrada.name}" usa el metodo de compresion ${entrada.method}: solo se soportan almacenado (0) y deflate (8)`
    );
  }
  const cabecera = entrada.localOffset;
  if (cabecera + 30 > buf.length) {
    return fallo(`la cabecera local de "${entrada.name}" queda fuera del archivo`);
  }
  if (buf.readUInt32LE(cabecera) !== LOCAL) {
    return fallo(`la cabecera local de "${entrada.name}" tiene una firma invalida`);
  }
  const nameLen = buf.readUInt16LE(cabecera + 26);
  const extraLen = buf.readUInt16LE(cabecera + 28);
  const inicio = cabecera + 30 + nameLen + extraLen;
  const fin = inicio + entrada.compressedSize;
  if (fin > buf.length) {
    return fallo(`los datos de "${entrada.name}" estan truncados`);
  }
  const comprimido = buf.subarray(inicio, fin);
  let datos;
  if (entrada.method === ALMACENADO) {
    datos = Buffer.from(comprimido);
  } else {
    try {
      datos = (0, import_node_zlib.inflateRawSync)(comprimido);
    } catch (error) {
      return fallo(`no se pudo descomprimir "${entrada.name}": ${mensajeDe(error)}`);
    }
  }
  if (datos.length !== entrada.uncompressedSize) {
    return fallo(
      `"${entrada.name}" ocupa ${datos.length} bytes y el indice dice ${entrada.uncompressedSize}`
    );
  }
  const real = (0, import_node_zlib.crc32)(datos);
  if (real !== entrada.crc) {
    return fallo(
      `"${entrada.name}" esta danada: su CRC es ${real.toString(16)} y el indice dice ${entrada.crc.toString(16)}`
    );
  }
  return datos;
}
function nombreSeguro(name) {
  return (0, import_node_path3.basename)(name.replace(/\\/g, "/")).replace(/[<>:"|?*\u0000-\u001f]/g, "_");
}
function esBasura(name) {
  const normalizado = name.replace(/\\/g, "/");
  if (normalizado.endsWith("/")) return true;
  if (normalizado.startsWith("__MACOSX/")) return true;
  const base = (0, import_node_path3.basename)(normalizado);
  return base === "" || base.startsWith(".");
}
function esImagen(name, extensiones) {
  const ext = (0, import_node_path3.extname)(name).replace(/^\./, "").toLowerCase();
  return ext !== "" && extensiones.includes(ext);
}
function nombreUnico(name, usados) {
  if (!usados.has(name)) {
    usados.add(name);
    return name;
  }
  const ext = (0, import_node_path3.extname)(name);
  const raiz = ext === "" ? name : name.slice(0, -ext.length);
  let n = 2;
  while (usados.has(`${raiz} (${n})${ext}`)) n++;
  const unico = `${raiz} (${n})${ext}`;
  usados.add(unico);
  return unico;
}
async function expandArchive(request) {
  let buf;
  try {
    buf = await (0, import_promises2.readFile)(request.archivePath);
  } catch (error) {
    return fallo(`no se pudo leer el archivo: ${mensajeDe(error)}`);
  }
  const eocd = localizarEocd(buf);
  if (typeof eocd !== "number") return eocd;
  const entradas = leerDirectorioCentral(buf, eocd);
  if (!Array.isArray(entradas)) return entradas;
  const paginas = entradas.filter(
    (entrada) => !esBasura(entrada.name) && esImagen(entrada.name, request.imageExtensions)
  );
  if (paginas.length === 0) {
    return fallo(
      `el archivo no contiene ninguna imagen: ${entradas.length} entrada(s), ninguna con extension de imagen`,
      "SU-E001"
    );
  }
  try {
    await (0, import_promises2.mkdir)(request.destDir, { recursive: true });
  } catch (error) {
    return fallo(`no se pudo crear el directorio de extraccion: ${mensajeDe(error)}`);
  }
  const images = [];
  const usados = /* @__PURE__ */ new Set();
  for (const entrada of paginas) {
    const datos = extraerEntrada(buf, entrada);
    if (!Buffer.isBuffer(datos)) return datos;
    const destino = nombreUnico(nombreSeguro(entrada.name), usados);
    const path = (0, import_node_path3.join)(request.destDir, destino);
    try {
      await (0, import_promises2.writeFile)(path, datos);
    } catch (error) {
      return fallo(`no se pudo escribir "${destino}": ${mensajeDe(error)}`);
    }
    images.push({ name: entrada.name, path });
  }
  return { status: "expanded", images };
}

// apps/desktop/src/main/archives/service.ts
function esArchivoComprimido(path) {
  const ext = (0, import_node_path4.extname)(path).replace(/^\./, "").toLowerCase();
  return ARCHIVE_EXTENSIONS.includes(ext);
}
function directorioDe(path, root) {
  const huella = (0, import_node_crypto.createHash)("sha256").update(path).digest("hex").slice(0, 16);
  return (0, import_node_path4.join)(root, huella);
}
var ArchiveService = class {
  root;
  constructor(root = (0, import_node_path4.join)((0, import_node_os2.tmpdir)(), "superupscaly-archives")) {
    this.root = root;
  }
  isArchive(path) {
    return esArchivoComprimido(path);
  }
  /**
   * Sustituye cada archivo comprimido por las imagenes que contiene.
   *
   * Los elementos que ya son imagenes pasan tal cual y **conservan su posicion
   * relativa**: el orden del lote es el que eligio el usuario, y las paginas de
   * un CBZ entran donde estaba el CBZ.
   *
   * Los fallos se devuelven en vez de lanzarse, para que quien llama decida. No
   * se descarta ninguno en silencio: un archivo que el usuario pidio y que no
   * aparece en el resultado seria un fallo mudo.
   *
   * Un comprimido que no se pudo abrir **se queda en la lista, en su sitio**, y
   * ademas aparece en `failed`. Las dos cosas son necesarias: la interfaz necesita
   * una ruta que enseñar en la fila (si desapareciera, la lista cambiaria de
   * tamano bajo los dedos del usuario) y quien llama necesita el motivo para
   * contarlo. La lista resultante nunca es mas corta que la de entrada.
   */
  async expandAll(items) {
    const salida = [];
    const failed = [];
    for (const item of items) {
      if (!this.isArchive(item)) {
        salida.push(item);
        continue;
      }
      const resultado = await expandArchive({
        archivePath: item,
        destDir: directorioDe(item, this.root),
        imageExtensions: IMAGE_EXTENSIONS
      });
      if (resultado.status === "failed") {
        failed.push({ item, errorCode: resultado.errorCode, message: resultado.message });
        salida.push(item);
        continue;
      }
      for (const imagen of resultado.images) salida.push(imagen.path);
    }
    return { items: salida, failed };
  }
};

// apps/desktop/src/main/store/settings.ts
var import_electron2 = require("electron");
var import_node_fs3 = require("node:fs");
var import_node_path5 = require("node:path");
var cache = null;
function settingsPath() {
  return (0, import_node_path5.join)(import_electron2.app.getPath("userData"), "settings.json");
}
function loadSettings() {
  if (cache) return cache;
  const file = settingsPath();
  let stored = {};
  try {
    const raw = (0, import_node_fs3.readFileSync)(file, "utf8");
    const parsed = JSON.parse(raw);
    stored = sanitizeSettingsPatch(parsed);
  } catch (error) {
    const code = error.code;
    if (code !== "ENOENT") {
      logger.warn("settings.read-failed", { code: code ?? "unknown" });
      try {
        (0, import_node_fs3.renameSync)(file, `${file}.bak`);
      } catch {
      }
    }
  }
  cache = { ...DEFAULT_SETTINGS, ...stored };
  return cache;
}
function saveSettings(settings) {
  const file = settingsPath();
  const tmp = `${file}.tmp`;
  try {
    (0, import_node_fs3.mkdirSync)(import_electron2.app.getPath("userData"), { recursive: true });
    (0, import_node_fs3.writeFileSync)(tmp, `${JSON.stringify(settings, null, 2)}
`, "utf8");
    (0, import_node_fs3.renameSync)(tmp, file);
  } catch (error) {
    logger.error("settings.write-failed", { code: error.code ?? "unknown" });
  }
}
function updateSettings(patch) {
  const current = loadSettings();
  const clean = sanitizeSettingsPatch(patch);
  const next = { ...current, ...clean };
  cache = next;
  saveSettings(next);
  logger.debug("settings.updated", { keys: Object.keys(clean) });
  return next;
}

// apps/desktop/src/main/ipc/handle.ts
var import_electron3 = require("electron");
function isTrustedSender(event) {
  return import_electron3.BrowserWindow.getAllWindows().some(
    (win) => !win.isDestroyed() && win.webContents.id === event.sender.id
  );
}
function handle(channel, handler) {
  import_electron3.ipcMain.handle(channel, async (event, ...args) => {
    if (!isTrustedSender(event)) {
      logger.warn("ipc.untrusted-sender", { channel });
      throw new Error("IPC request from an untrusted sender");
    }
    try {
      return await handler(...args);
    } catch (error) {
      logger.warn("ipc.handler-failed", {
        channel,
        message: error instanceof Error ? error.message : String(error)
      });
      throw error;
    }
  });
}
function broadcast(channel, payload) {
  for (const win of import_electron3.BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) {
      win.webContents.send(channel, payload);
    }
  }
}

// apps/desktop/src/main/ipc/index.ts
function defaultOutputDir() {
  return (0, import_node_path6.join)(import_electron4.app.getPath("pictures"), "Upscaled");
}
var OPEN_PATH_TIMEOUT_MS = 1500;
async function openInFileManager(target) {
  let timer;
  const expired = new Promise((resolve) => {
    timer = setTimeout(() => {
      logger.debug("shell.open-path-sin-respuesta", { target });
      resolve(true);
    }, OPEN_PATH_TIMEOUT_MS);
  });
  try {
    const opened = import_electron4.shell.openPath(target).then((error) => error === "");
    return await Promise.race([opened, expired]);
  } catch (error) {
    logger.warn("shell.open-path-failed", {
      target,
      message: error instanceof Error ? error.message : String(error)
    });
    return false;
  } finally {
    if (timer !== void 0) clearTimeout(timer);
  }
}
function registerIpcHandlers() {
  handle(IPC.pathsValidate, async (rawPaths) => {
    return validatePaths(toSafePathArray(rawPaths, MAX_DROP_FILES));
  });
  handle(IPC.filesPick, async () => {
    const result = await import_electron4.dialog.showOpenDialog({
      title: "Seleccionar imagenes",
      properties: ["openFile", "multiSelections", "dontAddToRecent"],
      filters: [
        { name: "Imagenes y archivos", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "avif", "zip", "cbz"] },
        { name: "Todos los archivos", extensions: ["*"] }
      ]
    });
    if (result.canceled) return [];
    const validated = await validatePaths(result.filePaths);
    return validated.accepted;
  });
  handle(IPC.folderPick, async (rawTitle) => {
    const result = await import_electron4.dialog.showOpenDialog({
      title: isString(rawTitle) && rawTitle.length <= 120 ? rawTitle : "Seleccionar carpeta",
      properties: ["openDirectory", "createDirectory", "dontAddToRecent"]
    });
    const first = result.filePaths[0];
    return result.canceled || !first ? null : first;
  });
  handle(IPC.folderExpand, async (rawDir, rawRecursive) => {
    if (!isSafePathString(rawDir)) return { paths: [], scanned: 0, truncated: false };
    return expandFolder(rawDir, isBoolean(rawRecursive) ? rawRecursive : true);
  });
  const archives = new ArchiveService();
  handle(IPC.archivesExpand, async (rawPaths) => {
    const requested = toSafePathArray(rawPaths, MAX_DROP_FILES);
    const outcome = await archives.expandAll(requested);
    const known = new Set(requested);
    for (const path of outcome.items) {
      if (!known.has(path)) registerMediaPath(path);
    }
    if (outcome.failed.length > 0) {
      logger.warn("archives.expand-partial", {
        requested: requested.length,
        failed: outcome.failed.length,
        codes: outcome.failed.map((failure) => failure.errorCode)
      });
    }
    return {
      items: outcome.items,
      failed: outcome.failed.map((failure) => ({
        path: failure.item,
        errorCode: failure.errorCode,
        message: failure.message
      }))
    };
  });
  handle(IPC.outputDefault, () => defaultOutputDir());
  handle(IPC.dirEnsure, async (rawDir) => {
    if (!isSafePathString(rawDir)) return { ok: false, path: "" };
    try {
      await (0, import_promises3.mkdir)(rawDir, { recursive: true });
      await (0, import_promises3.access)(rawDir, import_node_fs4.constants.W_OK);
      return { ok: true, path: rawDir };
    } catch (error) {
      logger.warn("dir.ensure-failed", { code: error.code ?? "unknown" });
      return { ok: false, path: rawDir };
    }
  });
  handle(IPC.filesStat, async (rawPaths) => {
    return statFiles(toSafePathArray(rawPaths, MAX_STAT_FILES));
  });
  handle(IPC.settingsGet, () => loadSettings());
  handle(IPC.settingsSet, (patch) => updateSettings(patch));
  handle(IPC.appInfo, () => ({
    name: import_electron4.app.getName(),
    version: import_electron4.app.getVersion(),
    electron: process.versions.electron ?? "unknown",
    chrome: process.versions.chrome ?? "unknown",
    node: process.versions.node ?? "unknown",
    platform: toRuntimePlatform(process.platform),
    arch: process.arch,
    isPackaged: import_electron4.app.isPackaged
  }));
  handle(IPC.reveal, async (rawPath) => {
    if (!isSafePathString(rawPath)) return false;
    try {
      const info = await (0, import_promises3.stat)(rawPath);
      if (info.isDirectory()) {
        return await openInFileManager(rawPath);
      }
      import_electron4.shell.showItemInFolder(rawPath);
      return true;
    } catch {
      return false;
    }
  });
  handle(IPC.openLogs, async () => {
    const dir = getLogDirectory();
    if (!dir) return false;
    return openInFileManager(dir);
  });
  logger.info("ipc.registered");
}

// apps/desktop/src/main/ipc/sidecar.ts
var import_node_path9 = require("node:path");

// apps/desktop/src/main/models/service.ts
var import_node_path8 = require("node:path");

// apps/desktop/src/main/models/downloader.ts
var import_node_crypto2 = require("node:crypto");
var import_node_fs5 = require("node:fs");
var import_promises4 = require("node:fs/promises");
var import_node_path7 = require("node:path");
var PART_SUFFIX = ".part";
var DEFAULT_STALL_MS = 6e4;
var DEFAULT_PROGRESS_MS = 150;
async function downloadModel(request) {
  if (request.urls.length === 0) {
    return fail("SU-E112", "el manifiesto no declara ninguna URL para este modelo");
  }
  const partPath = `${request.destPath}${PART_SUFFIX}`;
  try {
    await (0, import_promises4.mkdir)((0, import_node_path7.dirname)(request.destPath), { recursive: true });
  } catch (error) {
    return fail("SU-E150", `no se pudo preparar el directorio de modelos: ${describe(error)}`);
  }
  let lastMessage = "no se intent\xF3 ninguna descarga";
  for (const url of request.urls) {
    if (request.signal?.aborted) return { status: "cancelled" };
    const attempt = await attemptMirror(request, url, partPath);
    if (attempt.status === "cancelled") return { status: "cancelled" };
    if (attempt.status === "failed") {
      lastMessage = `${url}: ${attempt.message}`;
      continue;
    }
    const actual = await sha256File(partPath).catch((error) => {
      lastMessage = `${url}: no se pudo leer lo descargado: ${describe(error)}`;
      return null;
    });
    if (actual === null) continue;
    if (actual !== request.sha256.toLowerCase()) {
      await (0, import_promises4.rm)(partPath, { force: true }).catch(() => void 0);
      return fail(
        "SU-E111",
        `el archivo descargado no coincide con el manifiesto (esperado ${request.sha256.slice(0, 12)}\u2026, obtenido ${actual.slice(0, 12)}\u2026)`
      );
    }
    try {
      await (0, import_promises4.rename)(partPath, request.destPath);
    } catch (error) {
      return fail("SU-E150", `no se pudo colocar el modelo en su sitio: ${describe(error)}`);
    }
    const bytes = await (0, import_promises4.stat)(request.destPath).then(
      (info) => info.size,
      () => attempt.receivedBytes
    );
    return { status: "completed", bytes };
  }
  if (request.signal?.aborted) return { status: "cancelled" };
  return fail("SU-E112", lastMessage);
}
async function attemptMirror(request, url, partPath) {
  const already = await (0, import_promises4.stat)(partPath).then(
    (info) => info.size,
    () => 0
  );
  if (already === 0) {
    return withoutStale(await fetchInto(request, url, partPath, 0));
  }
  const resumed = await fetchInto(request, url, partPath, already);
  if (resumed.status !== "stale") return resumed;
  await (0, import_promises4.rm)(partPath, { force: true }).catch(() => void 0);
  return withoutStale(await fetchInto(request, url, partPath, 0));
}
function withoutStale(outcome) {
  if (outcome.status === "stale") {
    return { status: "failed", message: "el servidor rechaz\xF3 reanudar la descarga (HTTP 416)" };
  }
  return outcome;
}
async function fetchInto(request, url, partPath, from) {
  const already = from;
  const controller = new AbortController();
  const onUserAbort = () => controller.abort();
  request.signal?.addEventListener("abort", onUserAbort, { once: true });
  let stalled = false;
  const stallMs = request.stallMs ?? DEFAULT_STALL_MS;
  let stallTimer;
  const armStallTimer = () => {
    clearTimeout(stallTimer);
    stallTimer = setTimeout(() => {
      stalled = true;
      controller.abort();
    }, stallMs);
  };
  const startedAt = Date.now();
  let received = already;
  let lastReportAt = 0;
  let total = request.sizeBytes;
  try {
    const headers = {};
    if (already > 0) headers["range"] = `bytes=${already}-`;
    armStallTimer();
    const response = await fetch(url, { headers, signal: controller.signal, redirect: "follow" });
    if (!response.ok) {
      if (response.status === 416 && already > 0) {
        return { status: "stale" };
      }
      return { status: "failed", message: `HTTP ${response.status}` };
    }
    const resuming = already > 0 && response.status === 206;
    if (already > 0 && !resuming) received = 0;
    const contentLength = Number(response.headers.get("content-length") ?? 0);
    if (total === 0) total = received + (Number.isFinite(contentLength) ? contentLength : 0);
    if (!response.body) {
      return { status: "failed", message: "la respuesta no trae cuerpo" };
    }
    const stream2 = (0, import_node_fs5.createWriteStream)(partPath, { flags: resuming ? "a" : "w" });
    const reader = response.body.getReader();
    try {
      for (; ; ) {
        armStallTimer();
        const { done, value } = await reader.read();
        clearTimeout(stallTimer);
        if (done) break;
        if (!value) continue;
        received += value.byteLength;
        await writeChunk(stream2, value);
        const now = Date.now();
        const progressMs = request.progressMs ?? DEFAULT_PROGRESS_MS;
        const isLast = total > 0 && received >= total;
        if (progressMs === 0 || isLast || now - lastReportAt >= progressMs) {
          lastReportAt = now;
          request.onProgress?.(progress(received, total, url, startedAt));
        }
      }
    } finally {
      clearTimeout(stallTimer);
      await reader.cancel().catch(() => void 0);
      await closeStream(stream2);
    }
    if (total > 0 && received < total) {
      return { status: "failed", message: `descarga incompleta (${received} de ${total} bytes)` };
    }
    return { status: "completed", receivedBytes: received };
  } catch (error) {
    if (stalled) {
      return { status: "failed", message: `sin datos durante ${stallMs} ms` };
    }
    if (request.signal?.aborted) return { status: "cancelled" };
    if (isAbort(error)) return { status: "cancelled" };
    return { status: "failed", message: describe(error) };
  } finally {
    clearTimeout(stallTimer);
    request.signal?.removeEventListener("abort", onUserAbort);
  }
}
function progress(receivedBytes, totalBytes, url, startedAt) {
  const elapsed = (Date.now() - startedAt) / 1e3;
  return {
    receivedBytes,
    totalBytes,
    url,
    ratio: totalBytes > 0 ? Math.min(1, receivedBytes / totalBytes) : null,
    bytesPerSecond: elapsed > 0.25 ? receivedBytes / elapsed : null
  };
}
function writeChunk(stream2, chunk) {
  return new Promise((resolve, reject) => {
    stream2.write(chunk, (error) => error ? reject(error) : resolve());
  });
}
function closeStream(stream2) {
  return new Promise((resolve, reject) => {
    stream2.end((error) => error ? reject(error) : resolve());
  });
}
async function sha256File(path) {
  const handle2 = await (0, import_promises4.open)(path, "r");
  const hash = (0, import_node_crypto2.createHash)("sha256");
  try {
    const buffer = Buffer.allocUnsafe(1024 * 1024);
    for (; ; ) {
      const { bytesRead } = await handle2.read(buffer, 0, buffer.length, null);
      if (bytesRead === 0) break;
      hash.update(buffer.subarray(0, bytesRead));
    }
  } finally {
    await handle2.close();
  }
  return hash.digest("hex");
}
function isAbort(error) {
  return typeof error === "object" && error !== null && "name" in error && error.name === "AbortError";
}
function describe(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}
function fail(errorCode, message) {
  return { status: "failed", errorCode, message };
}

// apps/desktop/src/main/models/service.ts
var ModelDownloadService = class {
  constructor(client) {
    this.client = client;
  }
  client;
  inFlight = /* @__PURE__ */ new Map();
  /**
   * Descarga un modelo. Si ya se está descargando, devuelve la misma promesa.
   *
   * Nunca rechaza: un fallo de red es un resultado, no una excepción, y la
   * interfaz necesita el código de error para poder explicarlo.
   */
  async download(modelId) {
    const existing = this.inFlight.get(modelId);
    if (existing) return existing.done;
    const controller = new AbortController();
    const done = this.run(modelId, controller).finally(() => {
      this.inFlight.delete(modelId);
    });
    this.inFlight.set(modelId, { controller, done });
    return done;
  }
  /** Cancela la descarga en curso de un modelo. `false` si no había ninguna. */
  cancel(modelId) {
    const running = this.inFlight.get(modelId);
    if (!running) return false;
    running.controller.abort();
    return true;
  }
  /** Cancela todo. Se llama al cerrar la aplicación. */
  cancelAll() {
    for (const running of this.inFlight.values()) running.controller.abort();
  }
  async run(modelId, controller) {
    const catalog = await this.client.models().catch((error) => {
      logger.warn("models.catalog-failed", { message: describe2(error) });
      return null;
    });
    if (catalog === null) {
      return this.emitFailed(modelId, "SU-E112", "no se pudo consultar el cat\xE1logo de modelos");
    }
    const model = catalog.models.find((entry) => entry.id === modelId);
    if (!model) {
      return this.emitFailed(modelId, "SU-E110", `el cat\xE1logo no conoce el modelo '${modelId}'`);
    }
    const download = model.download;
    if (!download || download.urls.length === 0) {
      return this.emitFailed(
        modelId,
        "SU-E112",
        "este modelo no declara de d\xF3nde descargarse"
      );
    }
    if (!isPlainFileName(download.fileName)) {
      return this.emitFailed(modelId, "SU-E161", `nombre de archivo no v\xE1lido: ${download.fileName}`);
    }
    const destPath = (0, import_node_path8.join)(catalog.modelsDir, download.fileName);
    this.emit({
      modelId,
      status: "started",
      receivedBytes: 0,
      totalBytes: download.sizeBytes,
      ratio: null,
      bytesPerSecond: null,
      url: download.urls[0] ?? "",
      errorCode: null
    });
    const result = await downloadModel({
      modelId,
      urls: download.urls,
      sha256: download.sha256,
      sizeBytes: download.sizeBytes,
      destPath,
      signal: controller.signal,
      onProgress: (progress2) => {
        this.emit({
          modelId,
          status: "progress",
          receivedBytes: progress2.receivedBytes,
          totalBytes: progress2.totalBytes,
          ratio: progress2.ratio,
          bytesPerSecond: progress2.bytesPerSecond,
          url: progress2.url,
          errorCode: null
        });
      }
    });
    if (result.status === "completed") {
      logger.info("models.downloaded", { modelId, bytes: result.bytes });
      return this.emit({
        modelId,
        status: "completed",
        receivedBytes: result.bytes,
        totalBytes: download.sizeBytes,
        ratio: 1,
        bytesPerSecond: null,
        url: download.urls[0] ?? "",
        errorCode: null
      });
    }
    if (result.status === "cancelled") {
      return this.emit({
        modelId,
        status: "cancelled",
        receivedBytes: 0,
        totalBytes: download.sizeBytes,
        ratio: null,
        bytesPerSecond: null,
        url: download.urls[0] ?? "",
        errorCode: null
      });
    }
    logger.warn("models.download-failed", { modelId, code: result.errorCode });
    return this.emit({
      modelId,
      status: "failed",
      receivedBytes: 0,
      totalBytes: download.sizeBytes,
      ratio: null,
      bytesPerSecond: null,
      url: download.urls[0] ?? "",
      errorCode: result.errorCode
    });
  }
  emitFailed(modelId, errorCode, message) {
    logger.warn("models.download-rejected", { modelId, code: errorCode, message });
    return this.emit({
      modelId,
      status: "failed",
      receivedBytes: 0,
      totalBytes: 0,
      ratio: null,
      bytesPerSecond: null,
      url: "",
      errorCode
    });
  }
  emit(event) {
    broadcast(IPC.modelDownloadEvent, event);
    return event;
  }
};
function isPlainFileName(name) {
  return name.length > 0 && name.length <= 200 && !name.includes("/") && !name.includes("\\") && !name.includes("..");
}
function describe2(error) {
  return error instanceof Error ? error.message : String(error);
}

// apps/desktop/src/main/models/ensure.ts
function nativeScaleOf(catalog, modelId) {
  return catalog.find((entry) => entry.id === modelId)?.scale;
}
function stageScales(stage, catalog) {
  if (stage.op !== "model" || !stage.model) return false;
  const scale = stage.scaleOut ?? nativeScaleOf(catalog, stage.model) ?? 1;
  return scale > 1;
}
function requiredModels(pipelines, catalog, request) {
  const pipeline = pipelines.find(
    (entry) => entry.mode === request.mode && entry.scale === request.scale
  );
  if (!pipeline) return [];
  const manual = request.modelChainMode === "manual" && request.upscaleModel && request.upscaleModel.length > 0 ? request.upscaleModel : null;
  const needed = [];
  for (const stage of pipeline.stages) {
    if (!stageScales(stage, catalog)) continue;
    const model = manual ?? stage.model;
    if (!needed.includes(model)) needed.push(model);
  }
  return needed;
}
function engineUsesModels(engine) {
  return !engine.toLowerCase().startsWith("clasico");
}
async function ensureModelsFor(source, request, download) {
  const report = {
    present: [],
    downloaded: [],
    unavailable: [],
    interpolated: false,
    catalogError: null
  };
  let capabilities;
  let catalog;
  let pipelines;
  try {
    capabilities = await source.capabilities();
    catalog = await source.models();
    pipelines = await source.pipelines();
  } catch (error) {
    return { ...report, catalogError: describe3(error) };
  }
  if (!engineUsesModels(capabilities.engine)) {
    report.interpolated = true;
    return report;
  }
  const needed = requiredModels(pipelines.pipelines, catalog.models, request);
  for (const modelId of needed) {
    const entry = catalog.models.find((model) => model.id === modelId);
    if (!entry) {
      report.unavailable.push({ modelId, reason: "el catalogo no conoce este modelo" });
      continue;
    }
    if (entry.state === "installed") {
      report.present.push(modelId);
      continue;
    }
    const urls = entry.download?.urls ?? [];
    if (urls.length === 0) {
      report.unavailable.push({
        modelId,
        reason: "el catalogo no declara de donde descargarlo"
      });
      continue;
    }
    const result = await download(modelId);
    if (result.status === "completed") {
      report.downloaded.push(modelId);
    } else {
      report.unavailable.push({
        modelId,
        reason: result.errorCode ?? result.status
      });
    }
  }
  return report;
}
function describe3(error) {
  return error instanceof Error ? error.message : String(error);
}

// apps/desktop/src/main/ipc/sidecar.ts
function registerSidecarIpc(supervisor, client) {
  handle(IPC.sidecarStatus, () => supervisor.getStatus());
  handle(IPC.sidecarCapabilities, () => client.capabilities());
  handle(IPC.sidecarModels, () => client.models());
  handle(IPC.sidecarPipelines, () => client.pipelines());
  const archives = new ArchiveService();
  const models = new ModelDownloadService(client);
  handle(IPC.sidecarCreateJob, async (raw) => {
    const request = sanitizeJobRequest(raw);
    const expandido = await archives.expandAll(request.items);
    if (expandido.failed.length > 0) {
      throw new Error(describirFallosDeArchivo(expandido.failed));
    }
    const ensured = await ensureModelsFor(
      client,
      {
        mode: request.mode,
        scale: request.scale,
        modelChainMode: request.options.modelChainMode,
        upscaleModel: request.options.upscaleModel
      },
      (modelId) => models.download(modelId)
    );
    if (ensured.downloaded.length > 0 || ensured.unavailable.length > 0 || ensured.catalogError) {
      logger.info("models.ensured", {
        mode: request.mode,
        scale: request.scale,
        downloaded: ensured.downloaded,
        present: ensured.present,
        unavailable: ensured.unavailable,
        catalogError: ensured.catalogError,
        interpolated: ensured.interpolated
      });
    }
    return client.createJob({ ...request, items: expandido.items });
  });
  handle(IPC.sidecarListJobs, () => client.listJobs());
  handle(
    IPC.sidecarGetJob,
    (raw) => client.getJob(requireId(raw, "trabajo"))
  );
  handle(
    IPC.sidecarPauseJob,
    (raw) => client.pauseJob(requireId(raw, "trabajo"))
  );
  handle(
    IPC.sidecarResumeJob,
    (raw) => client.resumeJob(requireId(raw, "trabajo"))
  );
  handle(
    IPC.sidecarCancelJob,
    (raw) => client.cancelJob(requireId(raw, "trabajo"))
  );
  handle(IPC.sidecarRestart, async () => {
    await supervisor.stop();
    return supervisor.start();
  });
  handle(
    IPC.modelDownload,
    (raw) => models.download(requireId(raw, "modelo"))
  );
  handle(
    IPC.modelDownloadCancel,
    (raw) => models.cancel(requireId(raw, "modelo"))
  );
  client.subscribe((event) => {
    if (event.type === "itemCompleted" && !registerMediaPath(event.outPath)) {
      logger.warn("media.output-path-rejected", { itemId: event.itemId });
    }
    broadcast(IPC.sidecarEvent, event);
  });
  supervisor.onStatusChange((status) => {
    broadcast(IPC.sidecarStatusChanged, status);
  });
  logger.info("ipc.sidecar-registered");
  return models;
}
function requireId(raw, what) {
  if (typeof raw !== "string" || raw.length === 0 || raw.length > 128) {
    throw new Error(`Identificador de ${what} no valido`);
  }
  return raw;
}
function describirFallosDeArchivo(fallos) {
  const partes = fallos.map((fallo2) => `${(0, import_node_path9.basename)(fallo2.item)}: ${fallo2.message}`);
  const cuantos = fallos.length === 1 ? "un archivo comprimido" : `${fallos.length} archivos comprimidos`;
  return `No se pudo abrir ${cuantos}. ${partes.join("; ")}`;
}
function sanitizeJobRequest(raw) {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("La peticion de trabajo no es un objeto");
  }
  const value = raw;
  const mode = value["mode"];
  if (mode !== "photo" && mode !== "illustration") {
    throw new Error(`Modo no valido: ${String(mode)}`);
  }
  const scale = value["scale"];
  if (scale !== 2 && scale !== 4 && scale !== 8) {
    throw new Error(`Escala no valida: ${String(scale)}`);
  }
  const items = value["items"];
  if (!Array.isArray(items) || items.length === 0) {
    throw new Error("El trabajo no tiene imagenes");
  }
  const paths = [];
  for (const entry of items) {
    if (!isSafePathString(entry)) {
      throw new Error("Hay una ruta de archivo no valida en el trabajo");
    }
    paths.push(entry);
  }
  const output = value["output"];
  if (typeof output !== "object" || output === null) {
    throw new Error("Falta la configuracion de salida");
  }
  const outputRecord = output;
  const dir = outputRecord["dir"];
  if (!isSafePathString(dir)) {
    throw new Error("La carpeta de salida no es valida");
  }
  const format = outputRecord["format"];
  if (format !== "png" && format !== "jpg" && format !== "webp") {
    throw new Error(`Formato de salida no valido: ${String(format)}`);
  }
  const options = value["options"];
  if (typeof options !== "object" || options === null) {
    throw new Error("Faltan las opciones del trabajo");
  }
  const optionRecord = options;
  return {
    mode,
    scale,
    items: paths,
    output: {
      dir,
      format,
      quality: clampNumber(outputRecord["quality"], 1, 100, 95),
      suffix: typeof outputRecord["suffix"] === "string" ? outputRecord["suffix"].slice(0, 32) : "",
      preserveMetadata: outputRecord["preserveMetadata"] !== false,
      zipOutput: outputRecord["zipOutput"] === true
    },
    options: {
      tileSize: normalizeTile(optionRecord["tileSize"]),
      device: optionRecord["device"] === "cpu" || optionRecord["device"] === "gpu" ? optionRecord["device"] : "auto",
      concurrency: clampNumber(optionRecord["concurrency"], 1, 8, 1),
      unloadBetweenImages: optionRecord["unloadBetweenImages"] === true,
      modelChainMode: optionRecord["modelChainMode"] === "manual" ? "manual" : "auto",
      upscaleModel: typeof optionRecord["upscaleModel"] === "string" && optionRecord["upscaleModel"].length > 0 ? optionRecord["upscaleModel"] : null,
      faceRestore: normalizeFaceRestore(optionRecord["faceRestore"]),
      denoise: normalizeDenoise(optionRecord["denoise"]),
      sharpen: optionRecord["sharpen"] === true
    },
    priority: clampNumber(value["priority"], 0, 9, 0)
  };
}
function clampNumber(raw, min, max, fallback) {
  if (typeof raw !== "number" || !Number.isFinite(raw)) return fallback;
  return Math.min(max, Math.max(min, Math.round(raw)));
}
function normalizeTile(raw) {
  if (raw === 256 || raw === 384 || raw === 512 || raw === 768 || raw === 1024) return raw;
  return "auto";
}
function normalizeFaceRestore(raw) {
  if (raw === "off" || raw === "low" || raw === "medium" || raw === "high") return raw;
  return "auto";
}
function normalizeDenoise(raw) {
  if (raw === "off" || raw === "on") return raw;
  return "auto";
}

// apps/desktop/src/main/protocol.ts
var import_electron5 = require("electron");
var import_node_path10 = require("node:path");
var import_node_url = require("node:url");

// apps/desktop/src/main/csp.ts
var import_node_crypto3 = require("node:crypto");
var BASE_DIRECTIVES = [
  "default-src 'self'",
  "img-src 'self' data: blob:",
  "style-src 'self' 'unsafe-inline'",
  "font-src 'self' data:",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'"
];
var INLINE_SCRIPT = /<script(?![^>]*\bsrc\s*=)[^>]*>([\s\S]*?)<\/script\s*>/gi;
function inlineScriptHashes(html) {
  const hashes = [];
  for (const match of html.matchAll(INLINE_SCRIPT)) {
    const body = match[1];
    if (body === void 0 || body.length === 0) continue;
    const digest = (0, import_node_crypto3.createHash)("sha256").update(body, "utf8").digest("base64");
    hashes.push(`'sha256-${digest}'`);
  }
  return hashes;
}
function contentSecurityPolicy(html) {
  const hashes = inlineScriptHashes(html);
  const scriptSrc = hashes.length > 0 ? `script-src 'self' ${hashes.join(" ")}` : "script-src 'self'";
  return [...BASE_DIRECTIVES, scriptSrc].join("; ");
}

// apps/desktop/src/main/protocol.ts
function registerAppScheme() {
  import_electron5.protocol.registerSchemesAsPrivileged([
    {
      scheme: APP_SCHEME,
      privileges: {
        standard: true,
        secure: true,
        supportFetchAPI: true,
        stream: true,
        corsEnabled: false
      }
    }
  ]);
}
function registerAppProtocolHandler(rendererDir) {
  const root = (0, import_node_path10.normalize)(rendererDir);
  import_electron5.protocol.handle(APP_SCHEME, async (request) => {
    let pathname;
    try {
      pathname = decodeURIComponent(new URL(request.url).pathname);
    } catch {
      return new Response("Bad request", { status: 400 });
    }
    if (isMediaPath(pathname)) {
      const requested = parseMediaUrl(request.url);
      if (requested === null || !isMediaAllowed(requested)) {
        logger.warn("protocol.media-denied", {
          name: requested === null ? "" : (0, import_node_path10.basename)(requested)
        });
        return new Response("Forbidden", { status: 403 });
      }
      return await serveMedia(requested);
    }
    if (pathname === "" || pathname === "/") pathname = "/index.html";
    const target = (0, import_node_path10.normalize)((0, import_node_path10.join)(root, pathname));
    if (target !== root && !target.startsWith(root + import_node_path10.sep)) {
      logger.warn("protocol.path-escape-blocked");
      return new Response("Forbidden", { status: 403 });
    }
    try {
      const response = await import_electron5.net.fetch((0, import_node_url.pathToFileURL)(target).toString());
      if (!target.toLowerCase().endsWith(".html")) {
        return response;
      }
      const html = await response.text();
      const headers = new Headers(response.headers);
      headers.set("Content-Security-Policy", contentSecurityPolicy(html));
      return new Response(html, { status: response.status, headers });
    } catch (error) {
      logger.error("protocol.serve-failed", {
        target,
        message: error instanceof Error ? error.message : String(error)
      });
      return new Response("Not found", { status: 404 });
    }
  });
  logger.info("protocol.registered", { root });
}
async function serveMedia(absolutePath) {
  try {
    const response = await import_electron5.net.fetch((0, import_node_url.pathToFileURL)(absolutePath).toString());
    if (!response.ok) {
      logger.warn("protocol.media-missing", { name: (0, import_node_path10.basename)(absolutePath), status: response.status });
      return new Response("Not found", { status: 404 });
    }
    const headers = new Headers(response.headers);
    headers.set("Cache-Control", "no-store");
    return new Response(response.body, { status: response.status, headers });
  } catch (error) {
    logger.error("protocol.media-failed", {
      name: (0, import_node_path10.basename)(absolutePath),
      message: error instanceof Error ? error.message : String(error)
    });
    return new Response("Not found", { status: 404 });
  }
}

// apps/desktop/src/main/sidecar/client.ts
var import_ws = require("ws");
var SidecarError = class extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
    this.name = "SidecarError";
  }
  code;
};
var REQUEST_TIMEOUT_MS = 15e3;
var RECONNECT_BASE_MS = 500;
var RECONNECT_MAX_MS = 1e4;
var SidecarClient = class {
  constructor(supervisor) {
    this.supervisor = supervisor;
  }
  supervisor;
  socket = null;
  reconnectDelay = RECONNECT_BASE_MS;
  reconnectTimer = null;
  closed = false;
  listeners = /* @__PURE__ */ new Set();
  /** `true` si el sidecar esta listo para atender peticiones. */
  isReady() {
    return this.supervisor.getStatus().state === "ready" && this.supervisor.getBaseUrl() !== null;
  }
  async request(path, init = {}) {
    const baseUrl = this.supervisor.getBaseUrl();
    if (!baseUrl) {
      throw new SidecarError("SU-E120", "El sidecar no esta disponible.");
    }
    let response;
    try {
      response = await fetch(`${baseUrl}${path}`, {
        ...init,
        headers: {
          ...init.headers,
          Authorization: `Bearer ${this.supervisor.getToken()}`,
          "Content-Type": "application/json"
        },
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
      });
    } catch (error) {
      throw new SidecarError(
        "SU-E131",
        `No se pudo contactar con el sidecar: ${error instanceof Error ? error.message : String(error)}`
      );
    }
    if (!response.ok) {
      throw await toSidecarError(response);
    }
    if (response.status === 204) {
      return void 0;
    }
    return await response.json();
  }
  async capabilities() {
    return this.request("/v1/capabilities");
  }
  async models() {
    const body = await this.request(
      "/v1/models"
    );
    return { modelsDir: body.modelsDir, models: body.models };
  }
  /**
   * Los pipelines que el motor va a usar de verdad.
   *
   * Se piden al sidecar en lugar de reproducir sus reglas aquí: cuál es el modelo
   * de una etapa es una decisión del motor, y la aplicación solo necesita saberlo
   * para descargarlo antes de aceptar el trabajo.
   */
  async pipelines() {
    return this.request("/v1/pipelines");
  }
  async createJob(request) {
    return this.request("/v1/jobs", {
      method: "POST",
      body: JSON.stringify(request)
    });
  }
  async listJobs() {
    return this.request("/v1/jobs");
  }
  async getJob(id) {
    return this.request(`/v1/jobs/${encodeURIComponent(id)}`);
  }
  async pauseJob(id) {
    return this.request(`/v1/jobs/${encodeURIComponent(id)}/pause`, { method: "POST" });
  }
  async resumeJob(id) {
    return this.request(`/v1/jobs/${encodeURIComponent(id)}/resume`, { method: "POST" });
  }
  async cancelJob(id) {
    return this.request(`/v1/jobs/${encodeURIComponent(id)}/cancel`, { method: "POST" });
  }
  /**
   * Se suscribe al flujo de eventos. Devuelve la funcion para darse de baja.
   *
   * La conexion se abre de forma perezosa: si no hay nadie escuchando, no hay
   * socket. Se cierra cuando se va el ultimo suscriptor.
   */
  subscribe(listener) {
    this.listeners.add(listener);
    this.closed = false;
    if (!this.socket) {
      this.openSocket();
    }
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) {
        this.closeSocket();
      }
    };
  }
  openSocket() {
    const baseUrl = this.supervisor.getBaseUrl();
    if (!baseUrl) {
      this.scheduleReconnect();
      return;
    }
    const url = `${baseUrl.replace("http://", "ws://")}/v1/events`;
    try {
      this.socket = new import_ws.WebSocket(url, {
        headers: { Authorization: `Bearer ${this.supervisor.getToken()}` }
      });
    } catch (error) {
      logger.warn("sidecar.ws-open-failed", {
        message: error instanceof Error ? error.message : String(error)
      });
      this.scheduleReconnect();
      return;
    }
    this.socket.on("open", () => {
      logger.info("sidecar.events-connected");
      this.reconnectDelay = RECONNECT_BASE_MS;
    });
    this.socket.on("message", (data) => {
      this.dispatch(data);
    });
    this.socket.on("close", () => {
      this.socket = null;
      if (!this.closed) {
        this.scheduleReconnect();
      }
    });
    this.socket.on("error", (error) => {
      logger.warn("sidecar.ws-error", { message: error.message });
    });
  }
  dispatch(raw) {
    const text = typeof raw === "string" ? raw : String(raw);
    let event;
    try {
      event = JSON.parse(text);
    } catch {
      logger.warn("sidecar.event-unparsable");
      return;
    }
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch (error) {
        logger.warn("sidecar.listener-failed", {
          message: error instanceof Error ? error.message : String(error)
        });
      }
    }
  }
  scheduleReconnect() {
    if (this.closed || this.reconnectTimer) return;
    const wait = this.reconnectDelay;
    this.reconnectDelay = Math.min(this.reconnectDelay * 2, RECONNECT_MAX_MS);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.closed && this.listeners.size > 0) {
        this.openSocket();
      }
    }, wait);
  }
  closeSocket() {
    this.closed = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.socket) {
      try {
        this.socket.close();
      } catch {
      }
      this.socket = null;
    }
  }
  /** Cierra la conexion y descarta a los suscriptores. */
  dispose() {
    this.listeners.clear();
    this.closeSocket();
  }
};
async function toSidecarError(response) {
  try {
    const body = await response.json();
    return new SidecarError(
      body.code ?? `SU-E${response.status}`,
      body.message ?? `El sidecar respondio ${response.status}`
    );
  } catch {
    return new SidecarError(`SU-E${response.status}`, `El sidecar respondio ${response.status}`);
  }
}

// apps/desktop/src/main/sidecar/supervisor.ts
var import_node_child_process = require("node:child_process");
var import_node_crypto4 = require("node:crypto");
var import_node_fs8 = require("node:fs");
var import_node_path13 = require("node:path");

// apps/desktop/src/main/sidecar/locator.ts
var import_node_fs6 = require("node:fs");
var import_node_path11 = require("node:path");
function binaryName(platform = process.platform) {
  return platform === "win32" ? "su-cli.exe" : "su-cli";
}
function candidatePaths(options) {
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const name = binaryName(platform);
  const candidates = [];
  if (options.override && options.override.length > 0) {
    candidates.push(options.override);
  }
  if (options.isPackaged) {
    candidates.push((0, import_node_path11.join)(options.resourcesPath, "bin", `${platform}-${arch}`, name));
    candidates.push((0, import_node_path11.join)(options.resourcesPath, "bin", name));
  } else {
    const root = (0, import_node_path11.join)(options.appPath, "..", "..");
    const target = (0, import_node_path11.join)(root, "services", "inference", "target");
    candidates.push((0, import_node_path11.join)(target, "release", name));
    candidates.push((0, import_node_path11.join)(target, "debug", name));
  }
  return candidates;
}
function locateSidecar(options) {
  const candidates = candidatePaths(options);
  for (const [index, candidate] of candidates.entries()) {
    if ((0, import_node_fs6.existsSync)(candidate)) {
      return {
        command: candidate,
        baseArgs: [],
        origin: index === 0 && options.override ? "variable de entorno" : candidate
      };
    }
  }
  return null;
}
function serveArgs(portfile, port = 0, dataDir) {
  const args = ["serve", "--port", String(port), "--portfile", portfile];
  const override = dataDir?.trim();
  return override && override.length > 0 ? ["--data-dir", override, ...args] : args;
}

// apps/desktop/src/main/sidecar/ort.ts
var import_node_fs7 = require("node:fs");
var import_node_path12 = require("node:path");
function runtimeLibraryNames(platform = process.platform) {
  if (platform === "win32") return ["onnxruntime.dll"];
  if (platform === "darwin") return ["libonnxruntime.dylib"];
  return ["libonnxruntime.so"];
}
function ortLibraryCandidates(options) {
  const platform = options.platform ?? process.platform;
  const names = runtimeLibraryNames(platform);
  const explicit = options.existing?.trim();
  if (explicit && explicit.length > 0) {
    return [explicit];
  }
  const directories = [(0, import_node_path12.join)(options.dataDir, "runtime"), (0, import_node_path12.dirname)(options.sidecarPath)];
  return directories.flatMap((directory) => names.map((name) => (0, import_node_path12.join)(directory, name)));
}
function runtimeStem(name, platform) {
  const lower = name.toLowerCase();
  if (lower.includes("providers")) {
    return null;
  }
  const stems = platform === "win32" ? ["onnxruntime"] : ["libonnxruntime", "onnxruntime"];
  const stem = stems.find((candidate) => lower.startsWith(candidate));
  if (!stem) return null;
  const isLibrary = lower.endsWith(".so") || lower.endsWith(".dylib") || lower.endsWith(".dll") || lower.includes(".so.");
  return isLibrary ? stem : null;
}
function pickRuntimeLibrary(names, platform = process.platform) {
  const candidates = [];
  for (const name of names) {
    const stem = runtimeStem(name, platform);
    if (!stem) continue;
    const lower = name.toLowerCase();
    const canonical = lower === `${stem}.so` || lower === `${stem}.dylib` || lower === `${stem}.dll`;
    candidates.push({ exact: canonical ? 0 : 1, length: name.length, name });
  }
  candidates.sort((left, right) => left.exact - right.exact || left.length - right.length);
  return candidates[0]?.name ?? null;
}
function resolveOrtLibrary(options) {
  const platform = options.platform ?? process.platform;
  for (const candidate of ortLibraryCandidates(options)) {
    if ((0, import_node_fs7.existsSync)(candidate)) return candidate;
  }
  if (options.existing && options.existing.trim().length > 0) return null;
  const directories = [(0, import_node_path12.join)(options.dataDir, "runtime"), (0, import_node_path12.dirname)(options.sidecarPath)];
  for (const directory of directories) {
    let entries;
    try {
      entries = (0, import_node_fs7.readdirSync)(directory);
    } catch {
      continue;
    }
    const chosen = pickRuntimeLibrary(entries, platform);
    if (chosen) return (0, import_node_path12.join)(directory, chosen);
  }
  return null;
}

// apps/desktop/src/main/sidecar/supervisor.ts
var DEFAULT_STARTUP_TIMEOUT_MS = 1e4;
var PORTFILE_POLL_MS = 150;
var BACKOFF_BASE_MS = 1e3;
var BACKOFF_MAX_MS = 3e4;
var SidecarSupervisor = class {
  constructor(options) {
    this.options = options;
  }
  options;
  child = null;
  token = "";
  runtime = null;
  location = null;
  stopping = false;
  restarts = 0;
  backoffMs = BACKOFF_BASE_MS;
  /** El hijo no llego a nacer (falta la biblioteca, no es ejecutable…). */
  spawnFailed = false;
  restartTimer = null;
  status = {
    state: "stopped",
    restarts: 0,
    missingBinary: false
  };
  listeners = /* @__PURE__ */ new Set();
  getStatus() {
    return this.status;
  }
  getRuntime() {
    return this.runtime;
  }
  /** Base de la API, o `null` si el sidecar no esta listo. */
  getBaseUrl() {
    return this.runtime ? `http://127.0.0.1:${this.runtime.port}` : null;
  }
  getToken() {
    return this.token;
  }
  onStatusChange(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
  setStatus(patch) {
    this.status = { ...this.status, ...patch, restarts: this.restarts };
    for (const listener of this.listeners) {
      listener(this.status);
    }
  }
  /**
   * Arranca el sidecar y espera a que responda.
   *
   * Un `start()` explicito reinicia los contadores de reinicio. Es lo que
   * distingue «vuelve a intentarlo tu» de «vuelve a intentarlo solo»: los
   * arranques automaticos (`scheduleRestart`) se espacian con backoff y se rinden
   * a los `MAX_SIDECAR_RESTARTS`, pero si el usuario pulsa Reiniciar tras cinco
   * caidas, lo que pide es un intento limpio, no que la aplicacion siga contando
   * y se niegue a intentarlo.
   */
  async start() {
    this.stopping = false;
    this.restarts = 0;
    this.backoffMs = BACKOFF_BASE_MS;
    this.location = locateSidecar(this.options);
    if (!this.location) {
      logger.warn("sidecar.binary-not-found");
      this.setStatus({
        state: "unavailable",
        missingBinary: true,
        detail: "No se encontro el ejecutable del sidecar. Compilalo con: cargo build --release -p su-cli"
      });
      return this.status;
    }
    return this.launch();
  }
  async launch() {
    if (!this.location) {
      return this.status;
    }
    this.spawnFailed = false;
    this.setStatus({
      state: this.restarts > 0 ? "restarting" : "starting",
      missingBinary: false,
      detail: void 0
    });
    this.token = (0, import_node_crypto4.randomBytes)(32).toString("hex");
    const portfile = (0, import_node_path13.join)(this.options.dataDir, "runtime.json");
    const ortLibrary = resolveOrtLibrary({
      sidecarPath: this.location.command,
      dataDir: this.options.dataDir,
      existing: process.env.ORT_DYLIB_PATH
    });
    if (ortLibrary) {
      logger.info("sidecar.ort-runtime", { library: ortLibrary });
    } else {
      logger.info("sidecar.ort-runtime-missing", {
        detail: "sin ONNX Runtime: el sidecar usara interpolacion clasica"
      });
    }
    const sidecarDataDir = process.env.SU_DATA_DIR?.trim();
    if (sidecarDataDir && sidecarDataDir.length > 0) {
      logger.info("sidecar.data-dir-override", { dataDir: sidecarDataDir });
    }
    const child = (0, import_node_child_process.spawn)(
      this.location.command,
      [
        ...this.location.baseArgs,
        ...serveArgs(portfile, 0, sidecarDataDir && sidecarDataDir.length > 0 ? sidecarDataDir : void 0)
      ],
      {
        env: {
          ...process.env,
          SU_TOKEN: this.token,
          ...ortLibrary ? { ORT_DYLIB_PATH: ortLibrary } : {}
        },
        // La salida del sidecar se reenvia al log en lugar de a la consola: en
        // una aplicacion empaquetada no hay consola que mirar.
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true
      }
    );
    this.child = child;
    child.stdout?.on("data", (chunk) => {
      logger.debug("sidecar.stdout", { line: chunk.toString().trim().slice(0, 400) });
    });
    child.stderr?.on("data", (chunk) => {
      logger.warn("sidecar.stderr", { line: chunk.toString().trim().slice(0, 400) });
    });
    child.on("error", (error) => {
      logger.error("sidecar.spawn-failed", { message: error.message });
      this.spawnFailed = true;
      this.setStatus({ state: "failed", detail: `No se pudo arrancar: ${error.message}` });
    });
    child.on("exit", (code, signal) => {
      this.child = null;
      this.runtime = null;
      if (this.stopping) {
        logger.info("sidecar.stopped", { code, signal });
        this.setStatus({ state: "stopped", port: void 0, detail: void 0 });
        return;
      }
      logger.warn("sidecar.exited-unexpectedly", { code, signal });
      this.scheduleRestart(code ?? -1);
    });
    const ready = await this.waitUntilReady(child, portfile);
    if (!ready) {
      return this.status;
    }
    return this.status;
  }
  /** Espera a que el sidecar publique el puerto y responda al health check. */
  async waitUntilReady(child, portfile) {
    const timeout = this.options.startupTimeoutMs ?? DEFAULT_STARTUP_TIMEOUT_MS;
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      if (this.spawnFailed) return false;
      if (child.exitCode !== null) {
        this.setStatus({
          state: "failed",
          detail: `El sidecar termino al arrancar (codigo ${child.exitCode}).`
        });
        return false;
      }
      const runtime = this.readPortfile(portfile, child.pid);
      if (runtime) {
        if (runtime.protocol !== SIDECAR_PROTOCOL_VERSION) {
          this.setStatus({
            state: "failed",
            detail: `Version de protocolo incompatible: el sidecar habla v${runtime.protocol} y la aplicacion espera v${SIDECAR_PROTOCOL_VERSION}.`
          });
          return false;
        }
        if (await this.healthCheck(runtime.port)) {
          this.runtime = runtime;
          this.backoffMs = BACKOFF_BASE_MS;
          this.setStatus({
            state: "ready",
            version: runtime.version,
            port: runtime.port,
            detail: void 0
          });
          logger.info("sidecar.ready", { port: runtime.port, version: runtime.version });
          return true;
        }
      }
      await delay(PORTFILE_POLL_MS);
    }
    this.setStatus({
      state: "failed",
      detail: `El sidecar no respondio en ${Math.round(timeout / 1e3)} s.`
    });
    return false;
  }
  /**
   * Lee el portfile solo si pertenece al proceso que acabamos de lanzar.
   *
   * Sin esta comprobacion, un `runtime.json` de una sesion anterior se leeria en
   * el primer intento y la aplicacion intentaria hablar con un puerto muerto.
   */
  readPortfile(portfile, expectedPid) {
    try {
      const parsed = JSON.parse((0, import_node_fs8.readFileSync)(portfile, "utf8"));
      if (typeof parsed !== "object" || parsed === null) return null;
      const candidate = parsed;
      if (typeof candidate.port !== "number" || typeof candidate.pid !== "number") {
        return null;
      }
      if (expectedPid !== void 0 && candidate.pid !== expectedPid) {
        return null;
      }
      if (typeof candidate.protocol !== "number" || typeof candidate.version !== "string") {
        return null;
      }
      return {
        port: candidate.port,
        pid: candidate.pid,
        version: candidate.version,
        protocol: candidate.protocol,
        startedAt: candidate.startedAt ?? ""
      };
    } catch {
      return null;
    }
  }
  async healthCheck(port) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/v1/health`, {
        signal: AbortSignal.timeout(2e3)
      });
      return response.ok;
    } catch {
      return false;
    }
  }
  scheduleRestart(exitCode) {
    if (this.restarts >= MAX_SIDECAR_RESTARTS) {
      this.setStatus({
        state: "failed",
        detail: `El sidecar se ha caido ${this.restarts} veces. Se deja de reintentar.`
      });
      logger.error("sidecar.giving-up", { restarts: this.restarts });
      return;
    }
    this.restarts += 1;
    const wait = this.backoffMs;
    this.backoffMs = Math.min(this.backoffMs * 2, BACKOFF_MAX_MS);
    this.setStatus({
      state: "restarting",
      detail: `El sidecar se cerro (codigo ${exitCode}). Reintentando en ${Math.round(wait / 1e3)} s\u2026`
    });
    this.restartTimer = setTimeout(() => {
      this.restartTimer = null;
      void this.launch();
    }, wait);
  }
  /**
   * Cierre ordenado: se pide al sidecar que se apague, se le da margen y solo
   * entonces se le envia una senal.
   *
   * Matarlo directamente dejaria el trabajo en curso sin marcar como
   * interrumpido, y la reanudacion no lo ofreceria.
   */
  async stop() {
    this.stopping = true;
    if (this.restartTimer) {
      clearTimeout(this.restartTimer);
      this.restartTimer = null;
    }
    const child = this.child;
    if (!child) {
      this.setStatus({ state: "stopped" });
      return;
    }
    const baseUrl = this.getBaseUrl();
    if (baseUrl) {
      try {
        await fetch(`${baseUrl}/v1/shutdown`, {
          method: "POST",
          headers: { Authorization: `Bearer ${this.token}` },
          signal: AbortSignal.timeout(2e3)
        });
      } catch {
      }
    }
    if (await waitForExit(child, 3e3)) {
      return;
    }
    logger.warn("sidecar.forcing-termination");
    child.kill("SIGTERM");
    if (await waitForExit(child, 2e3)) {
      return;
    }
    child.kill("SIGKILL");
  }
};
function delay(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}
function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      child.removeListener("exit", onExit);
      resolve(false);
    }, timeoutMs);
    const onExit = () => {
      clearTimeout(timer);
      resolve(true);
    };
    child.once("exit", onExit);
  });
}

// apps/desktop/src/main/windows.ts
var import_electron6 = require("electron");
var import_node_fs9 = require("node:fs");
var import_node_path14 = require("node:path");
var DEFAULT_SIZE = { width: 1280, height: 860 };
var MIN_SIZE = { width: 1024, height: 700 };
function statePath() {
  return (0, import_node_path14.join)(import_electron6.app.getPath("userData"), "window-state.json");
}
function loadWindowState() {
  const fallback = { ...DEFAULT_SIZE, x: null, y: null, maximized: false };
  try {
    const parsed = JSON.parse((0, import_node_fs9.readFileSync)(statePath(), "utf8"));
    if (typeof parsed !== "object" || parsed === null) return fallback;
    const candidate = parsed;
    const state = {
      width: typeof candidate.width === "number" ? candidate.width : DEFAULT_SIZE.width,
      height: typeof candidate.height === "number" ? candidate.height : DEFAULT_SIZE.height,
      x: typeof candidate.x === "number" ? candidate.x : null,
      y: typeof candidate.y === "number" ? candidate.y : null,
      maximized: candidate.maximized === true
    };
    if (state.x !== null && state.y !== null) {
      const visible = import_electron6.screen.getAllDisplays().some((display) => {
        const { x, y, width, height } = display.workArea;
        return state.x !== null && state.y !== null && state.x < x + width && state.y < y + height && state.x + state.width > x && state.y + state.height > y;
      });
      if (!visible) {
        state.x = null;
        state.y = null;
      }
    }
    return state;
  } catch {
    return fallback;
  }
}
function saveWindowState(win) {
  if (win.isDestroyed()) return;
  try {
    const bounds = win.getNormalBounds();
    const state = {
      width: bounds.width,
      height: bounds.height,
      x: bounds.x,
      y: bounds.y,
      maximized: win.isMaximized()
    };
    (0, import_node_fs9.mkdirSync)(import_electron6.app.getPath("userData"), { recursive: true });
    (0, import_node_fs9.writeFileSync)(statePath(), `${JSON.stringify(state, null, 2)}
`, "utf8");
  } catch (error) {
    logger.warn("window.state-save-failed", { code: error.code ?? "unknown" });
  }
}
function createMainWindow(options) {
  const state = loadWindowState();
  const win = new import_electron6.BrowserWindow({
    width: state.width,
    height: state.height,
    ...state.x !== null && state.y !== null ? { x: state.x, y: state.y } : {},
    minWidth: MIN_SIZE.width,
    minHeight: MIN_SIZE.height,
    show: false,
    backgroundColor: "#1E1B2E",
    autoHideMenuBar: true,
    title: "SuperUpscaly",
    ...process.platform === "darwin" ? { titleBarStyle: "hiddenInset" } : {},
    webPreferences: {
      preload: (0, import_node_path14.join)(__dirname, "..", "preload", "index.js"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      spellcheck: false,
      devTools: !import_electron6.app.isPackaged
    }
  });
  if (state.maximized) win.maximize();
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith("https://")) void import_electron6.shell.openExternal(url);
    return { action: "deny" };
  });
  win.webContents.on("will-navigate", (event, url) => {
    const isDevServer = options.devUrl !== void 0 && url.startsWith(options.devUrl);
    if (!url.startsWith(APP_ORIGIN) && !isDevServer) {
      event.preventDefault();
      logger.warn("window.navigation-blocked", { url });
    }
  });
  win.webContents.on("render-process-gone", (_event, details) => {
    logger.error("window.render-process-gone", { reason: details.reason, exitCode: details.exitCode });
    if (details.reason !== "clean-exit" && !win.isDestroyed()) {
      win.reload();
    }
  });
  win.on("unresponsive", () => logger.warn("window.unresponsive"));
  win.on("responsive", () => logger.info("window.responsive"));
  win.webContents.on("console-message", (details) => {
    const context = {
      message: details.message.slice(0, 600),
      source: details.sourceId,
      line: details.lineNumber
    };
    if (details.level === "error") {
      logger.error("renderer.console-error", context);
    } else if (details.level === "warning") {
      logger.warn("renderer.console-warning", context);
    }
  });
  win.webContents.on("preload-error", (_event, preloadPath, error) => {
    logger.error("renderer.preload-error", { preloadPath, message: error.message });
  });
  win.webContents.on("did-fail-load", (_event, errorCode, errorDescription, url, isMainFrame) => {
    if (isMainFrame) {
      logger.error("renderer.load-failed", { errorCode, errorDescription, url });
    }
  });
  win.once("ready-to-show", () => {
    win.show();
    logger.info("window.shown");
  });
  win.on("close", () => saveWindowState(win));
  win.on("closed", () => logger.info("window.closed"));
  if (options.devUrl) {
    void win.loadURL(options.devUrl);
    win.webContents.openDevTools({ mode: "detach" });
    logger.info("window.load-dev-url", { url: options.devUrl });
  } else if (options.rendererDir) {
    void win.loadURL(`${APP_ORIGIN}/index.html`);
    logger.info("window.load-app-scheme");
  } else {
    logger.error("window.no-renderer-source");
    void win.loadURL(
      'data:text/html,<body style="font-family:sans-serif;background:%231E1B2E;color:%23F3F4F6;padding:40px"><h1>No se encontro el renderer</h1><p>Ejecuta <code>npm run build</code> antes de iniciar.</p></body>'
    );
  }
  return win;
}

// apps/desktop/src/main/index.ts
import_electron7.app.setName("SuperUpscaly");
registerAppScheme();
var hasSingleInstanceLock = import_electron7.app.requestSingleInstanceLock();
if (!hasSingleInstanceLock) {
  import_electron7.app.quit();
} else {
  let mainWindow = null;
  import_electron7.app.on("second-instance", () => {
    if (!mainWindow || mainWindow.isDestroyed()) return;
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.focus();
    logger.info("app.second-instance-focused");
  });
  import_electron7.app.on("web-contents-created", (_event, contents) => {
    contents.on("will-attach-webview", (event) => event.preventDefault());
  });
  void import_electron7.app.whenReady().then(() => {
    initLogger();
    logger.info("app.starting", {
      version: import_electron7.app.getVersion(),
      electron: process.versions.electron ?? "unknown",
      platform: process.platform,
      arch: process.arch,
      packaged: import_electron7.app.isPackaged
    });
    loadSettings();
    registerIpcHandlers();
    const supervisor = new SidecarSupervisor({
      isPackaged: import_electron7.app.isPackaged,
      resourcesPath: process.resourcesPath,
      appPath: import_electron7.app.getAppPath(),
      dataDir: import_electron7.app.getPath("userData"),
      override: process.env["SU_SIDECAR_BIN"]
    });
    const client = new SidecarClient(supervisor);
    const modelDownloads = registerSidecarIpc(supervisor, client);
    void supervisor.start();
    let sidecarReleased = false;
    const releaseDeadlineMs = 2500;
    import_electron7.app.on("before-quit", (event) => {
      modelDownloads.cancelAll();
      if (sidecarReleased) return;
      event.preventDefault();
      const deadline = new Promise((resolve) => {
        setTimeout(resolve, releaseDeadlineMs);
      });
      void Promise.race([supervisor.stop(), deadline]).finally(() => {
        sidecarReleased = true;
        import_electron7.app.quit();
      });
    });
    const devUrl = process.env["SU_DEV_URL"];
    const rendererDir = process.env["SU_RENDERER_DIR"] ?? (import_electron7.app.isPackaged ? (0, import_node_path15.join)(process.resourcesPath, "renderer") : void 0);
    if (rendererDir) {
      registerAppProtocolHandler(rendererDir);
    }
    mainWindow = createMainWindow({
      devUrl: devUrl && devUrl.length > 0 ? devUrl : void 0,
      rendererDir: devUrl ? void 0 : rendererDir
    });
    import_electron7.app.on("activate", () => {
      if (import_electron7.BrowserWindow.getAllWindows().length === 0) {
        mainWindow = createMainWindow({
          devUrl: devUrl && devUrl.length > 0 ? devUrl : void 0,
          rendererDir: devUrl ? void 0 : rendererDir
        });
      }
    });
    applyMenu();
  });
  import_electron7.app.on("window-all-closed", () => {
    if (process.platform !== "darwin") import_electron7.app.quit();
  });
  import_electron7.app.on("before-quit", () => {
    logger.info("app.quitting");
  });
  process.on("uncaughtException", (error) => {
    logger.error("app.uncaught-exception", { message: error.message, stack: error.stack ?? "" });
  });
  process.on("unhandledRejection", (reason) => {
    logger.error("app.unhandled-rejection", { reason: String(reason) });
  });
}
function applyMenu() {
  if (process.platform !== "darwin") {
    import_electron7.Menu.setApplicationMenu(null);
    return;
  }
  const menu = import_electron7.Menu.buildFromTemplate([
    {
      label: import_electron7.app.getName(),
      submenu: [
        { role: "about" },
        { type: "separator" },
        { role: "hide" },
        { role: "hideOthers" },
        { role: "unhide" },
        { type: "separator" },
        { role: "quit" }
      ]
    },
    {
      label: "Editar",
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "selectAll" }
      ]
    },
    {
      label: "Ventana",
      submenu: [{ role: "minimize" }, { role: "zoom" }, { role: "togglefullscreen" }]
    }
  ]);
  import_electron7.Menu.setApplicationMenu(menu);
}
//# sourceMappingURL=index.js.map
