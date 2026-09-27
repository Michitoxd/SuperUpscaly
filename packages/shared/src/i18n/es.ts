/**
 * Diccionario en espanol. Es la fuente de verdad de las claves de traduccion:
 * `Dictionary` se deriva de este objeto, asi que anadir una clave aqui y
 * olvidarla en `en.ts` produce un error de compilacion.
 */

export const es = {
  app: {
    name: 'SuperUpscaly',
    tagline: 'Upscaling local con IA',
  },

  common: {
    close: 'Cerrar',
    cancel: 'Cancelar',
    retry: 'Reintentar',
    auto: 'Automatico',
    on: 'Activado',
    off: 'Desactivado',
    seconds: 's',
    minutes: 'min',
    hours: 'h',
    clear: 'Vaciar lista',
    remove: 'Quitar',
    loading: 'Cargando…',
  },

  mode: {
    title: 'Modo',
    photo: 'Fotos',
    illustration: 'Dibujo / Anime',
    recommended: 'Recomendado',
    hint: 'Determina la cadena de modelos que se aplicara.',
  },

  scale: {
    title: 'Escala',
  },

  models: {
    title: 'Modelos',
    subtitle: 'Se descargan en el primer uso desde su origen original.',
    open: 'Modelos',
    empty: 'El motor aun no ha publicado su catalogo.',
    missingCount: '{count} sin descargar',
    download: 'Descargar',
    redownload: 'Volver a descargar',
    downloading: 'Descargando',
    openFolder: 'Abrir carpeta',
    sizeUnknown: 'tamano desconocido',
    noDownload: 'Sin origen declarado: instala este modelo a mano en la carpeta.',
    state: {
      installed: 'Instalado',
      missing: 'Sin descargar',
      hashMismatch: 'Archivo incorrecto',
      unverified: 'Sin verificar',
    },
    kind: {
      photo: 'Foto',
      illustration: 'Dibujo',
      denoise: 'Ruido',
      face: 'Rostros',
      detector: 'Deteccion',
      classifier: 'Clasificacion',
    },
  },

  drop: {
    title: 'Arrastra y suelta tus imagenes aqui',
    hint: 'PNG · JPG · WEBP · BMP · TIFF · AVIF · ZIP / CBZ',
    button: 'Seleccionar imagen(es)',
    dragging: 'Suelta para anadir',
    rejected: 'No se pudieron leer {count} archivo(s).',
    rejectedAction: 'Seleccionalos con el boton de archivos.',
    limitExceeded: 'Se ignoraron {count} archivo(s): el maximo por tanda es {max}.',
    archiveFailed: 'No se pudo abrir {count} archivo(s) comprimido(s).',
    emptyFolder: 'La carpeta no contiene imagenes compatibles.',
  },

  output: {
    title: 'Carpeta de salida',
    change: 'Cambiar',
    pickTitle: 'Elige la carpeta de salida',
    notWritable: 'No se puede escribir en esa carpeta.',
  },

  action: {
    upscaly: 'Upscaly',
    cancel: 'Cancelar',
    pause: 'Pausar',
    resume: 'Reanudar',
    openOutput: 'Abrir carpeta de salida',
    exportLogs: 'Exportar diagnostico',
  },

  advanced: {
    title: 'Avanzado',
    collapsedHint: 'Tile, dispositivo, cadena de modelos, restauracion facial',
    noBackend: 'El motor de escalado no esta listo: no se enviara ningun trabajo.',

    tile: {
      label: 'Tamano de tile',
      hint: 'En Automatico se calcula a partir de la VRAM libre. Forzarlo solo si sabes lo que haces.',
    },
    device: {
      label: 'Dispositivo',
      gpu: 'GPU',
      cpu: 'CPU',
      hint: 'En Automatico se usa la mejor GPU disponible y se cae a CPU si no hay ninguna.',
    },
    models: {
      label: 'Cadena de modelos',
      manual: 'Manual',
      upscale: 'Modelo base',
      hint: 'En Automatico la app elige el modelo segun el modo y el analisis de la imagen.',
    },
    face: {
      label: 'Restauracion facial',
      low: 'Suave',
      medium: 'Media',
      high: 'Fuerte',
      hint: 'Solo se aplica sobre los rostros detectados.',
      photoOnly: 'Solo disponible en modo Fotos.',
    },
    denoise: {
      label: 'Reduccion de ruido',
      hint: 'Se ejecuta antes del escalado. Automatico lo activa si la imagen esta degradada.',
    },
    sharpen: {
      label: 'Enfoque final',
      hint: 'Se inhibe automaticamente en imagenes con artefactos de compresion.',
    },
    concurrency: {
      label: 'Imagenes simultaneas',
      hint: '1 por GPU es el valor optimo: dos sesiones no aceleran y duplican la VRAM.',
    },
    unload: {
      label: 'Liberar modelo entre imagenes',
      hint: 'Reduce el pico de VRAM a costa de recargar el modelo cada vez.',
    },
    format: {
      label: 'Formato de salida',
      quality: 'Calidad',
      suffix: 'Sufijo del archivo',
      preserveMetadata: 'Conservar metadatos',
    },
  },

  queue: {
    title: 'Cola',
    empty: 'Sin imagenes en la cola',
    emptyHint: 'Arrastra archivos o usa el boton de seleccion.',
    colName: 'Archivo',
    colSize: 'Tamano',
    colStatus: 'Estado',
    colProgress: 'Progreso',
    colTime: 'Tiempo',
    clear: 'Vaciar lista',
  },

  status: {
    pending: 'En espera',
    running: 'Procesando',
    done: 'Completado',
    degraded: 'Degradado',
    failed: 'Fallido',
    skipped: 'Omitido',
  },

  progress: {
    title: 'Procesando',
    global: 'Progreso global',
    eta: 'Tiempo restante',
    preparingModel: 'Preparando: descargando el modelo',
    stage: {
      decode: 'Decodificando',
      analyze: 'Analizando',
      denoise: 'Reduciendo ruido',
      upscale: 'Escalando',
      face: 'Restaurando rostros',
      sharpen: 'Enfocando',
      encode: 'Guardando',
    },
  },

  summary: {
    title: 'Resumen',
    success: 'Completadas',
    failed: 'Fallidas',
    degraded: 'Degradadas',
    totalTime: 'Tiempo total',
    avgTime: 'Media por imagen',
    notes: 'Que se hizo distinto',
    notesHint:
      'Una etapa puede omitirse si su condicion no se cumple, o usar el modelo de reserva si el suyo no esta instalado. Aqui aparece lo que paso en cada imagen.',
  },

  compare: {
    title: 'Comparar antes y despues',
    open: 'Comparar',
    original: 'Original',
    upscaled: 'Escalado',
    /** Nombre accesible de la linea que se arrastra. */
    divider: 'Linea de comparacion',
    hint: 'Arrastra la linea sobre la imagen. Con el teclado, las flechas la mueven.',
    dimensions: '{width} x {height} px',
    failedOriginal:
      'No se puede previsualizar el original: este formato (TIFF, BMP) no se dibuja en la ventana.',
    failedUpscaled: 'No se puede previsualizar el resultado: es demasiado grande para mostrarlo aqui.',
    failedHint: 'El archivo si esta guardado: abrelo desde la carpeta de salida.',
    openFolder: 'Abrir carpeta',
  },

  error: {
    title: 'Se produjo un error',
    details: 'Detalle tecnico',
    copy: 'Copiar detalle',
    openLogs: 'Abrir carpeta de logs',
    actionLabel: 'Que puedes hacer',
  },

  settings: {
    language: 'Idioma',
    spanish: 'Espanol',
    english: 'Ingles',
  },

  /** La pagina se abrio fuera de Electron: no hay puente y no se puede escalar. */
  noBridge: {
    body: 'Esta pagina se esta mostrando fuera de la aplicacion, asi que no puede hablar con el motor. Abrela con `npm start`.',
  },

  sidecar: {
    title: 'Motor de escalado',
    restart: 'Reiniciar',
    restarting: 'Reiniciando el motor…',
    missingBinary:
      'No se encontro el ejecutable del sidecar. Compilalo con: cargo build --release -p su-cli',
    notReady: 'El motor de escalado no esta disponible todavia.',
    jobRejected: 'El motor rechazo el trabajo.',
    device: 'Dispositivo',
    cores: 'Nucleos',
    state: {
      stopped: 'Detenido',
      starting: 'Arrancando',
      ready: 'Listo',
      restarting: 'Reiniciando',
      failed: 'Con error',
      unavailable: 'No disponible',
    },
  },

  errors: {
    'SU-E001': {
      message: 'No se encontro ninguna imagen compatible.',
      action: 'Comprueba que los archivos son imagenes o archivos ZIP/CBZ validos.',
    },
    'SU-E100': {
      message: 'No se pudo decodificar el archivo.',
      action: 'Prueba a abrirlo en otro programa para descartar que este danado.',
    },
    'SU-E101': {
      message: 'El formato de la imagen no esta soportado.',
      action: 'Convierte la imagen a PNG, JPG, WEBP, BMP o TIFF.',
    },
    'SU-E102': {
      message: 'El archivo esta corrupto o incompleto.',
      action: 'Vuelve a copiarlo desde el origen.',
    },
    'SU-E110': {
      message: 'Falta el modelo necesario para este modo.',
      action: 'Abre el gestor de modelos y descargalo.',
    },
    'SU-E111': {
      message: 'El modelo descargado no coincide con el hash esperado.',
      action: 'Elimina el modelo y vuelve a descargarlo.',
    },
    'SU-E112': {
      message: 'No se pudo descargar el modelo desde su origen.',
      action: 'Comprueba la conexion y reintenta. Si usas un proxy, configuralo en los ajustes.',
    },
    'SU-E120': {
      message: 'La aceleracion por hardware no esta disponible; se usa un modo mas lento.',
      action: 'Actualiza el driver de la grafica si quieres recuperar el rendimiento completo.',
    },
    'SU-E121': {
      message: 'No se pudo compilar el motor de TensorRT.',
      action: 'Se usara CUDA en su lugar. Actualizar el driver suele resolverlo.',
    },
    'SU-E130': {
      message: 'Se agoto la memoria de la grafica.',
      action: 'Se reintento con tiles mas pequenos. Reduce el tile o cierra otras aplicaciones con GPU.',
    },
    'SU-E131': {
      message: 'Se perdio la conexion con el dispositivo grafico.',
      action: 'Guarda tu trabajo y reinicia la aplicacion.',
    },
    'SU-E140': {
      message: 'Fallo el procesado de un fragmento de la imagen.',
      action: 'Se reintento con una configuracion mas conservadora.',
    },
    'SU-E141': {
      message: 'El resultado no supero la validacion y no se guardo.',
      action: 'Prueba con otro modelo o reduce el factor de escala.',
    },
    'SU-E142': {
      message: 'A esa escala la imagen resultante superaria el limite de megapixeles.',
      action: 'Baja el factor de escala o sube el limite de megapixeles en los ajustes avanzados.',
    },
    'SU-E143': {
      message: 'El resultado no alcanzo la escala que promete el pipeline.',
      action: 'Es un fallo interno del pipeline, no de tu imagen. Informa de este codigo en el repositorio.',
    },
    'SU-E150': {
      message: 'No se pudo escribir el archivo de salida.',
      action: 'Comprueba los permisos y el espacio libre en disco.',
    },
    'SU-E160': {
      message: 'Operacion cancelada.',
      action: 'Puedes reanudarla cuando quieras.',
    },
    'SU-E161': {
      message: 'No se pudo obtener la ruta del archivo arrastrado.',
      action: 'Usa el boton de seleccionar archivos en su lugar.',
    },
    'SU-E900': {
      message: 'Error interno inesperado.',
      action: 'Exporta el diagnostico y adjuntalo al informe de error.',
    },
  },
} as const

/**
 * `es` esta declarado con `as const` para que las claves queden como literales.
 * `DeepString` ensancha solo los valores a `string`, de modo que `en.ts` debe
 * tener exactamente las mismas claves pero puede tener cualquier texto.
 */
export type DeepString<T> = {
  [K in keyof T]: T[K] extends string ? string : DeepString<T[K]>
}

export type Dictionary = DeepString<typeof es>
