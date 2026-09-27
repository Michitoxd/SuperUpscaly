/**
 * Taxonomia de errores de SuperUpscaly.
 *
 * Regla de oro del proyecto: ningun fallo puede ser silencioso. Todo error que
 * llega a la UI tiene un codigo estable, un mensaje traducido y una accion
 * sugerida. El codigo es lo unico que se registra en logs y telemetria; nunca
 * rutas de archivo ni contenido.
 */

export const ERROR_CODES = [
  'SU-E001',
  'SU-E100',
  'SU-E101',
  'SU-E102',
  'SU-E110',
  'SU-E111',
  'SU-E112',
  'SU-E120',
  'SU-E121',
  'SU-E130',
  'SU-E131',
  'SU-E140',
  'SU-E141',
  'SU-E142',
  'SU-E143',
  'SU-E150',
  'SU-E160',
  'SU-E161',
  'SU-E900',
] as const

export type ErrorCode = (typeof ERROR_CODES)[number]

export interface ErrorDescriptor {
  code: ErrorCode
  /** Identificador estable en ingles, para logs y busquedas. */
  slug: string
  /** true si el sistema puede recuperarse sin intervencion del usuario. */
  recoverable: boolean
  /** true si el usuario puede resolverlo siguiendo la accion sugerida. */
  userActionable: boolean
}

export const ERROR_CATALOG: Record<ErrorCode, ErrorDescriptor> = {
  'SU-E001': { code: 'SU-E001', slug: 'NoImagesFound', recoverable: false, userActionable: true },
  'SU-E100': { code: 'SU-E100', slug: 'DecodeFailed', recoverable: false, userActionable: true },
  'SU-E101': { code: 'SU-E101', slug: 'UnsupportedFormat', recoverable: false, userActionable: true },
  'SU-E102': { code: 'SU-E102', slug: 'CorruptFile', recoverable: false, userActionable: true },
  'SU-E110': { code: 'SU-E110', slug: 'ModelMissing', recoverable: false, userActionable: true },
  'SU-E111': { code: 'SU-E111', slug: 'ModelHashMismatch', recoverable: false, userActionable: true },
  'SU-E112': { code: 'SU-E112', slug: 'ModelDownloadFailed', recoverable: false, userActionable: true },
  'SU-E120': { code: 'SU-E120', slug: 'ExecutionProviderUnavailable', recoverable: true, userActionable: false },
  'SU-E121': { code: 'SU-E121', slug: 'TensorRTEngineBuildFailed', recoverable: true, userActionable: false },
  'SU-E130': { code: 'SU-E130', slug: 'OutOfVram', recoverable: true, userActionable: true },
  'SU-E131': { code: 'SU-E131', slug: 'DeviceLost', recoverable: true, userActionable: true },
  'SU-E140': { code: 'SU-E140', slug: 'TileFailed', recoverable: true, userActionable: false },
  'SU-E141': { code: 'SU-E141', slug: 'OutputValidationFailed', recoverable: false, userActionable: false },
  'SU-E142': { code: 'SU-E142', slug: 'OutputTooLarge', recoverable: false, userActionable: true },
  'SU-E143': { code: 'SU-E143', slug: 'ScaleNotReached', recoverable: false, userActionable: false },
  'SU-E150': { code: 'SU-E150', slug: 'WriteFailed', recoverable: false, userActionable: true },
  'SU-E160': { code: 'SU-E160', slug: 'Cancelled', recoverable: true, userActionable: false },
  'SU-E161': { code: 'SU-E161', slug: 'PathUnavailable', recoverable: false, userActionable: true },
  'SU-E900': { code: 'SU-E900', slug: 'Internal', recoverable: false, userActionable: true },
}

export function isErrorCode(value: unknown): value is ErrorCode {
  return typeof value === 'string' && (ERROR_CODES as readonly string[]).includes(value)
}

/** Clave de traduccion del mensaje de un codigo, p. ej. `errors.SU-E130.message`. */
export function errorMessageKey(code: ErrorCode): string {
  return `errors.${code}.message`
}

/** Clave de traduccion de la accion sugerida para un codigo. */
export function errorActionKey(code: ErrorCode): string {
  return `errors.${code}.action`
}
