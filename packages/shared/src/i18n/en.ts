import type { Dictionary } from './es.ts'

/**
 * English dictionary. Typed as `Dictionary`, so a missing or misspelled key is a
 * compile error. Keep the tone of the Spanish original: direct, no filler.
 */
export const en: Dictionary = {
  app: {
    name: 'SuperUpscaly',
    tagline: 'Local AI upscaling',
  },

  common: {
    close: 'Close',
    cancel: 'Cancel',
    retry: 'Retry',
    auto: 'Automatic',
    on: 'On',
    off: 'Off',
    seconds: 's',
    minutes: 'min',
    hours: 'h',
    clear: 'Clear list',
    remove: 'Remove',
    loading: 'Loading…',
  },

  mode: {
    title: 'Mode',
    photo: 'Photos',
    illustration: 'Art / Anime',
    recommended: 'Recommended',
    hint: 'Determines which model chain is applied.',
  },

  scale: {
    title: 'Scale',
  },

  models: {
    title: 'Models',
    subtitle: 'Downloaded on first use from their original source.',
    open: 'Models',
    empty: 'The engine has not published its catalogue yet.',
    missingCount: '{count} not downloaded',
    download: 'Download',
    redownload: 'Download again',
    downloading: 'Downloading',
    openFolder: 'Open folder',
    sizeUnknown: 'unknown size',
    noDownload: 'No declared source: install this model by hand into the folder.',
    state: {
      installed: 'Installed',
      missing: 'Not downloaded',
      hashMismatch: 'Wrong file',
      unverified: 'Unverified',
    },
    kind: {
      photo: 'Photo',
      illustration: 'Art',
      denoise: 'Noise',
      face: 'Faces',
      detector: 'Detection',
      classifier: 'Classification',
    },
  },

  drop: {
    title: 'Drag and drop your images here',
    hint: 'PNG · JPG · WEBP · BMP · TIFF · AVIF · ZIP / CBZ',
    button: 'Select image(s)',
    dragging: 'Drop to add',
    rejected: 'Could not read {count} file(s).',
    rejectedAction: 'Pick them with the file button instead.',
    limitExceeded: 'Ignored {count} file(s): the per-batch limit is {max}.',
    archiveFailed: 'Could not open {count} archive(s).',
    emptyFolder: 'The folder contains no supported images.',
  },

  output: {
    title: 'Output folder',
    change: 'Change',
    pickTitle: 'Choose the output folder',
    notWritable: 'That folder is not writable.',
  },

  action: {
    upscaly: 'Upscaly',
    cancel: 'Cancel',
    pause: 'Pause',
    resume: 'Resume',
    openOutput: 'Open output folder',
    exportLogs: 'Export diagnostics',
  },

  advanced: {
    title: 'Advanced',
    collapsedHint: 'Tile, device, model chain, face restoration',
    noBackend: 'The scaling engine is not ready: no job will be sent.',

    tile: {
      label: 'Tile size',
      hint: 'On Automatic it is derived from free VRAM. Override only if you know what you are doing.',
    },
    device: {
      label: 'Device',
      gpu: 'GPU',
      cpu: 'CPU',
      hint: 'On Automatic the best available GPU is used, falling back to CPU.',
    },
    models: {
      label: 'Model chain',
      manual: 'Manual',
      upscale: 'Base model',
      hint: 'On Automatic the app picks the model from the mode and the image analysis.',
    },
    face: {
      label: 'Face restoration',
      low: 'Soft',
      medium: 'Medium',
      high: 'Strong',
      hint: 'Only applied to detected faces.',
      photoOnly: 'Only available in Photos mode.',
    },
    denoise: {
      label: 'Noise reduction',
      hint: 'Runs before upscaling. Automatic enables it when the image is degraded.',
    },
    sharpen: {
      label: 'Final sharpening',
      hint: 'Automatically skipped on images with compression artefacts.',
    },
    concurrency: {
      label: 'Simultaneous images',
      hint: '1 per GPU is optimal: two sessions do not speed things up and double VRAM use.',
    },
    unload: {
      label: 'Unload model between images',
      hint: 'Lowers peak VRAM at the cost of reloading the model every time.',
    },
    format: {
      label: 'Output format',
      quality: 'Quality',
      suffix: 'File suffix',
      preserveMetadata: 'Preserve metadata',
    },
  },

  queue: {
    title: 'Queue',
    empty: 'No images in the queue',
    emptyHint: 'Drop files or use the select button.',
    colName: 'File',
    colSize: 'Size',
    colStatus: 'Status',
    colProgress: 'Progress',
    colTime: 'Time',
    clear: 'Clear list',
  },

  status: {
    pending: 'Waiting',
    running: 'Processing',
    done: 'Completed',
    degraded: 'Degraded',
    failed: 'Failed',
    skipped: 'Skipped',
  },

  progress: {
    title: 'Processing',
    global: 'Overall progress',
    eta: 'Time remaining',
    preparingModel: 'Preparing: downloading the model',
    stage: {
      decode: 'Decoding',
      analyze: 'Analysing',
      denoise: 'Denoising',
      upscale: 'Upscaling',
      face: 'Restoring faces',
      sharpen: 'Sharpening',
      encode: 'Saving',
    },
  },

  summary: {
    title: 'Summary',
    success: 'Completed',
    failed: 'Failed',
    degraded: 'Degraded',
    totalTime: 'Total time',
    avgTime: 'Average per image',
    notes: 'What ran differently',
    notesHint:
      'A stage can be skipped when its condition is not met, or fall back to the reserve model when its own is not installed. Here is what happened to each image.',
  },

  compare: {
    title: 'Compare before and after',
    open: 'Compare',
    original: 'Original',
    upscaled: 'Upscaled',
    divider: 'Comparison line',
    hint: 'Drag the line across the image. With the keyboard, the arrow keys move it.',
    dimensions: '{width} x {height} px',
    failedOriginal:
      'The original cannot be previewed: this format (TIFF, BMP) is not drawn in the window.',
    failedUpscaled: 'The result cannot be previewed: it is too large to show here.',
    failedHint: 'The file is saved: open it from the output folder.',
    openFolder: 'Open folder',
  },

  error: {
    title: 'Something went wrong',
    details: 'Technical detail',
    copy: 'Copy detail',
    openLogs: 'Open logs folder',
    actionLabel: 'What you can do',
  },

  settings: {
    language: 'Language',
    spanish: 'Spanish',
    english: 'English',
  },

  /** The page was opened outside Electron: there is no bridge and nothing can be scaled. */
  noBridge: {
    body: 'This page is being shown outside the application, so it cannot talk to the engine. Open it with `npm start`.',
  },

  sidecar: {
    title: 'Upscaling engine',
    restart: 'Restart',
    restarting: 'Restarting the engine…',
    missingBinary:
      'The sidecar executable was not found. Build it with: cargo build --release -p su-cli',
    notReady: 'The upscaling engine is not available yet.',
    jobRejected: 'The engine rejected the job.',
    device: 'Device',
    cores: 'Cores',
    state: {
      stopped: 'Stopped',
      starting: 'Starting',
      ready: 'Ready',
      restarting: 'Restarting',
      failed: 'Failed',
      unavailable: 'Unavailable',
    },
  },

  errors: {
    'SU-E001': {
      message: 'No supported image was found.',
      action: 'Make sure the files are valid images or ZIP/CBZ archives.',
    },
    'SU-E100': {
      message: 'The file could not be decoded.',
      action: 'Try opening it in another program to rule out corruption.',
    },
    'SU-E101': {
      message: 'This image format is not supported.',
      action: 'Convert the image to PNG, JPG, WEBP, BMP or TIFF.',
    },
    'SU-E102': {
      message: 'The file is corrupt or incomplete.',
      action: 'Copy it again from the source.',
    },
    'SU-E110': {
      message: 'The model required for this mode is missing.',
      action: 'Open the model manager and download it.',
    },
    'SU-E111': {
      message: 'The downloaded model does not match the expected hash.',
      action: 'Delete the model and download it again.',
    },
    'SU-E112': {
      message: 'The model could not be downloaded from its origin.',
      action: 'Check your connection and try again. If you are behind a proxy, configure it in the settings.',
    },
    'SU-E120': {
      message: 'Hardware acceleration is unavailable; a slower mode is being used.',
      action: 'Update your graphics driver to recover full performance.',
    },
    'SU-E121': {
      message: 'The TensorRT engine could not be built.',
      action: 'CUDA will be used instead. Updating the driver usually fixes this.',
    },
    'SU-E130': {
      message: 'Graphics memory ran out.',
      action: 'It was retried with smaller tiles. Lower the tile size or close other GPU apps.',
    },
    'SU-E131': {
      message: 'The graphics device connection was lost.',
      action: 'Save your work and restart the application.',
    },
    'SU-E140': {
      message: 'Processing a fragment of the image failed.',
      action: 'It was retried with a more conservative configuration.',
    },
    'SU-E141': {
      message: 'The result failed validation and was not saved.',
      action: 'Try another model or a lower scale factor.',
    },
    'SU-E142': {
      message: 'At that scale the result would go over the megapixel limit.',
      action: 'Lower the scale factor or raise the megapixel limit in the advanced settings.',
    },
    'SU-E143': {
      message: 'The result did not reach the scale the pipeline promises.',
      action: 'This is a pipeline bug, not a problem with your image. Please report this code in the repository.',
    },
    'SU-E150': {
      message: 'The output file could not be written.',
      action: 'Check permissions and free disk space.',
    },
    'SU-E160': {
      message: 'Operation cancelled.',
      action: 'You can resume it whenever you want.',
    },
    'SU-E161': {
      message: 'The path of the dropped file could not be obtained.',
      action: 'Use the select-files button instead.',
    },
    'SU-E900': {
      message: 'Unexpected internal error.',
      action: 'Export the diagnostics and attach them to the bug report.',
    },
  },
}
