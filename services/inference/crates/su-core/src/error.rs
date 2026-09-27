//! Taxonomia de errores de SuperUpscaly.
//!
//! Los codigos son identicos a los de `packages/shared/src/error-codes.ts`. Es un
//! contrato entre el sidecar y la interfaz: si aqui se anade un codigo, alli hay
//! que anadir su traduccion. Un test comprueba que ambos catalogos no se separan
//! (ver `tests/error_catalog.rs` en este mismo crate).
//!
//! Regla del proyecto: ningun fallo es silencioso. Todo error lleva codigo,
//! mensaje y una pista de si el usuario puede hacer algo al respecto.

use std::fmt;

/// Codigo estable de error. Se registra en logs y telemetria; nunca se
/// registran rutas ni contenido de imagen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ErrorCode {
    /// No se encontro ninguna imagen compatible.
    NoImagesFound,
    /// Fallo la decodificacion del archivo.
    DecodeFailed,
    /// El formato no esta soportado.
    UnsupportedFormat,
    /// El archivo esta corrupto o incompleto.
    CorruptFile,
    /// Falta el modelo necesario.
    ModelMissing,
    /// El hash del modelo no coincide con el manifiesto.
    ModelHashMismatch,
    /// No se pudo traer el modelo desde su origen.
    ///
    /// La descarga la hace la aplicacion, no este sidecar, pero el codigo vive
    /// aqui porque la taxonomia es una sola: el usuario ve el mismo `SU-E112`
    /// mire donde mire, y el catalogo no se parte en dos.
    ModelDownloadFailed,
    /// El execution provider no esta disponible.
    ExecutionProviderUnavailable,
    /// No se pudo construir el motor de TensorRT.
    TensorRtEngineBuildFailed,
    /// Se agoto la memoria de la GPU.
    OutOfVram,
    /// Se perdio el dispositivo grafico.
    DeviceLost,
    /// Fallo el procesado de un fragmento.
    TileFailed,
    /// El resultado no supero la validacion y no se guardo.
    OutputValidationFailed,
    /// El resultado pedido no cabe en el limite de megapixeles configurado.
    OutputTooLarge,
    /// El pipeline no llego a la escala que promete su identificador.
    ScaleNotReached,
    /// No se pudo escribir la salida.
    WriteFailed,
    /// Operacion cancelada por el usuario.
    Cancelled,
    /// No se pudo obtener la ruta del archivo.
    PathUnavailable,
    /// Error interno inesperado.
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoImagesFound => "SU-E001",
            Self::DecodeFailed => "SU-E100",
            Self::UnsupportedFormat => "SU-E101",
            Self::CorruptFile => "SU-E102",
            Self::ModelMissing => "SU-E110",
            Self::ModelHashMismatch => "SU-E111",
            Self::ModelDownloadFailed => "SU-E112",
            Self::ExecutionProviderUnavailable => "SU-E120",
            Self::TensorRtEngineBuildFailed => "SU-E121",
            Self::OutOfVram => "SU-E130",
            Self::DeviceLost => "SU-E131",
            Self::TileFailed => "SU-E140",
            Self::OutputValidationFailed => "SU-E141",
            Self::OutputTooLarge => "SU-E142",
            Self::ScaleNotReached => "SU-E143",
            Self::WriteFailed => "SU-E150",
            Self::Cancelled => "SU-E160",
            Self::PathUnavailable => "SU-E161",
            Self::Internal => "SU-E900",
        }
    }

    /// Identificador estable en ingles, para busquedas en logs.
    pub const fn slug(self) -> &'static str {
        match self {
            Self::NoImagesFound => "NoImagesFound",
            Self::DecodeFailed => "DecodeFailed",
            Self::UnsupportedFormat => "UnsupportedFormat",
            Self::CorruptFile => "CorruptFile",
            Self::ModelMissing => "ModelMissing",
            Self::ModelHashMismatch => "ModelHashMismatch",
            Self::ModelDownloadFailed => "ModelDownloadFailed",
            Self::ExecutionProviderUnavailable => "ExecutionProviderUnavailable",
            Self::TensorRtEngineBuildFailed => "TensorRtEngineBuildFailed",
            Self::OutOfVram => "OutOfVram",
            Self::DeviceLost => "DeviceLost",
            Self::TileFailed => "TileFailed",
            Self::OutputValidationFailed => "OutputValidationFailed",
            Self::OutputTooLarge => "OutputTooLarge",
            Self::ScaleNotReached => "ScaleNotReached",
            Self::WriteFailed => "WriteFailed",
            Self::Cancelled => "Cancelled",
            Self::PathUnavailable => "PathUnavailable",
            Self::Internal => "Internal",
        }
    }

    /// `true` si el sistema puede recuperarse sin intervencion del usuario
    /// (por ejemplo, reintentando con un tile mas pequeno).
    pub const fn recoverable(self) -> bool {
        matches!(
            self,
            Self::ExecutionProviderUnavailable
                | Self::TensorRtEngineBuildFailed
                | Self::OutOfVram
                | Self::DeviceLost
                | Self::TileFailed
                | Self::Cancelled
        )
    }

    /// `true` si el usuario puede resolverlo siguiendo una accion concreta.
    pub const fn user_actionable(self) -> bool {
        matches!(
            self,
            Self::NoImagesFound
                | Self::DecodeFailed
                | Self::UnsupportedFormat
                | Self::CorruptFile
                | Self::ModelMissing
                | Self::ModelHashMismatch
                // Reintentar la descarga, o mirar la conexion, es cosa del
                // usuario: el sistema no puede adivinar si hay red.
                | Self::ModelDownloadFailed
                | Self::OutOfVram
                | Self::DeviceLost
                // El limite de megapixeles se resuelve bajando la escala o
                // eligiendo una imagen mas pequena: eso lo decide el usuario.
                | Self::OutputTooLarge
                | Self::WriteFailed
                | Self::PathUnavailable
                | Self::Internal
        )
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error de dominio. El `Display` es el mensaje tecnico que va al log; la UI
/// muestra su propio texto traducido a partir de `code()`.
#[derive(Debug, thiserror::Error)]
pub enum SuError {
    #[error("no supported image was found")]
    NoImagesFound,

    #[error("could not decode image: {0}")]
    DecodeFailed(String),

    #[error("unsupported image format: {0}")]
    UnsupportedFormat(String),

    #[error("file is corrupt or incomplete: {0}")]
    CorruptFile(String),

    #[error("model not available: {0}")]
    ModelMissing(String),

    #[error("model hash mismatch for {model}: expected {expected}, got {actual}")]
    ModelHashMismatch {
        model: String,
        expected: String,
        actual: String,
    },

    #[error("could not download model {model} from {url}: {reason}")]
    ModelDownloadFailed {
        model: String,
        url: String,
        reason: String,
    },

    #[error("execution provider unavailable: {provider} ({reason})")]
    ExecutionProviderUnavailable { provider: String, reason: String },

    #[error("TensorRT engine build failed: {0}")]
    TensorRtEngineBuildFailed(String),

    /// Lleva el contexto necesario para decidir el reintento: con que tile fallo
    /// y con cuanta VRAM se contaba.
    #[error("out of VRAM with tile {tile} (free {free_mb} MiB, needed ~{needed_mb} MiB)")]
    OutOfVram {
        tile: u32,
        free_mb: u64,
        needed_mb: u64,
    },

    #[error("graphics device lost: {0}")]
    DeviceLost(String),

    #[error("tile {row},{col} failed: {reason}")]
    TileFailed {
        row: u32,
        col: u32,
        reason: String,
    },

    #[error("output failed validation ({reason}); nothing was written")]
    OutputValidationFailed { reason: String },

    /// El resultado pedido no cabe en el limite configurado. Se comprueba **antes**
    /// de empezar: llegar al final y devolver otro tamano seria peor que un error,
    /// porque el usuario guardaria un archivo equivocado sin saberlo.
    #[error(
        "x{scale} on this image would produce {estimated_mp:.0} MP, over the {limit_mp:.0} MP limit"
    )]
    OutputTooLarge {
        scale: u32,
        estimated_mp: f32,
        limit_mp: f32,
    },

    /// El pipeline no llego a la escala que promete su identificador. Es un fallo
    /// de autoria del pipeline (una etapa de escalado omitida, un factor mal
    /// puesto), no algo que el usuario pueda resolver.
    #[error(
        "pipeline promised x{expected_scale} ({expected_width}x{expected_height}) but produced {actual_width}x{actual_height}: {reason}"
    )]
    ScaleNotReached {
        expected_scale: u32,
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
        reason: String,
    },

    #[error("could not write output: {0}")]
    WriteFailed(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("path unavailable: {0}")]
    PathUnavailable(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl SuError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::NoImagesFound => ErrorCode::NoImagesFound,
            Self::DecodeFailed(_) => ErrorCode::DecodeFailed,
            Self::UnsupportedFormat(_) => ErrorCode::UnsupportedFormat,
            Self::CorruptFile(_) => ErrorCode::CorruptFile,
            Self::ModelMissing(_) => ErrorCode::ModelMissing,
            Self::ModelHashMismatch { .. } => ErrorCode::ModelHashMismatch,
            Self::ModelDownloadFailed { .. } => ErrorCode::ModelDownloadFailed,
            Self::ExecutionProviderUnavailable { .. } => ErrorCode::ExecutionProviderUnavailable,
            Self::TensorRtEngineBuildFailed(_) => ErrorCode::TensorRtEngineBuildFailed,
            Self::OutOfVram { .. } => ErrorCode::OutOfVram,
            Self::DeviceLost(_) => ErrorCode::DeviceLost,
            Self::TileFailed { .. } => ErrorCode::TileFailed,
            Self::OutputValidationFailed { .. } => ErrorCode::OutputValidationFailed,
            Self::OutputTooLarge { .. } => ErrorCode::OutputTooLarge,
            Self::ScaleNotReached { .. } => ErrorCode::ScaleNotReached,
            Self::WriteFailed(_) => ErrorCode::WriteFailed,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::PathUnavailable(_) => ErrorCode::PathUnavailable,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    pub fn recoverable(&self) -> bool {
        self.code().recoverable()
    }
}

pub type SuResult<T> = Result<T, SuError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_prefixed() {
        let all = [
            ErrorCode::NoImagesFound,
            ErrorCode::DecodeFailed,
            ErrorCode::UnsupportedFormat,
            ErrorCode::CorruptFile,
            ErrorCode::ModelMissing,
            ErrorCode::ModelHashMismatch,
            ErrorCode::ModelDownloadFailed,
            ErrorCode::ExecutionProviderUnavailable,
            ErrorCode::TensorRtEngineBuildFailed,
            ErrorCode::OutOfVram,
            ErrorCode::DeviceLost,
            ErrorCode::TileFailed,
            ErrorCode::OutputValidationFailed,
            ErrorCode::OutputTooLarge,
            ErrorCode::ScaleNotReached,
            ErrorCode::WriteFailed,
            ErrorCode::Cancelled,
            ErrorCode::PathUnavailable,
            ErrorCode::Internal,
        ];

        let mut seen = std::collections::HashSet::new();
        for code in all {
            assert!(code.as_str().starts_with("SU-E"), "{code} no tiene prefijo");
            assert!(seen.insert(code.as_str()), "codigo duplicado: {code}");
        }
        assert_eq!(seen.len(), 19);
    }

    #[test]
    fn a_failed_download_names_what_failed_and_is_the_users_to_retry() {
        let err = SuError::ModelDownloadFailed {
            model: "4x-ultrasharp".to_string(),
            url: "https://ejemplo.invalido/a.onnx".to_string(),
            reason: "conexion rechazada".to_string(),
        };

        assert_eq!(err.code().as_str(), "SU-E112");
        assert_eq!(err.code().slug(), "ModelDownloadFailed");
        // El sistema no puede adivinar si hay red, asi que no se declara
        // recuperable solo: reintentar es una decision del usuario.
        assert!(!err.code().recoverable());
        assert!(err.code().user_actionable());

        // El mensaje tiene que servir para diagnosticar sin abrir el codigo.
        let text = err.to_string();
        assert!(text.contains("4x-ultrasharp"), "{text}");
        assert!(text.contains("https://ejemplo.invalido/a.onnx"), "{text}");
        assert!(text.contains("conexion rechazada"), "{text}");
    }

    #[test]
    fn out_of_vram_is_recoverable_but_not_user_fixable_only() {
        let err = SuError::OutOfVram {
            tile: 512,
            free_mb: 900,
            needed_mb: 1400,
        };
        assert_eq!(err.code().as_str(), "SU-E130");
        assert!(err.recoverable());
        assert!(err.code().user_actionable());
    }

    #[test]
    fn output_validation_failure_is_not_recoverable() {
        let err = SuError::OutputValidationFailed {
            reason: "uniform buffer".into(),
        };
        assert_eq!(err.code().as_str(), "SU-E141");
        assert!(!err.recoverable());
    }

    #[test]
    fn an_output_over_the_limit_is_the_users_decision_to_fix() {
        // Reintentar con el mismo tamano daria el mismo resultado, asi que no es
        // recuperable; pero el usuario puede bajarlo, asi que si es accionable.
        let err = SuError::OutputTooLarge {
            scale: 8,
            estimated_mp: 6_400.0,
            limit_mp: 800.0,
        };
        assert_eq!(err.code().as_str(), "SU-E142");
        assert!(!err.recoverable());
        assert!(err.code().user_actionable());
        assert!(err.to_string().contains("6400 MP"), "{err}");
    }

    #[test]
    fn a_pipeline_that_misses_its_scale_is_nobody_elses_fault_to_fix() {
        // Es un fallo de autoria del pipeline: el usuario no puede hacer nada, y
        // reintentar tampoco lo arregla. Por eso se distingue de SU-E142.
        let err = SuError::ScaleNotReached {
            expected_scale: 8,
            expected_width: 512,
            expected_height: 512,
            actual_width: 128,
            actual_height: 128,
            reason: "upscale2: la condicion no se cumple".into(),
        };
        assert_eq!(err.code().as_str(), "SU-E143");
        assert!(!err.recoverable());
        assert!(!err.code().user_actionable());
        assert!(err.to_string().contains("512x512"), "{err}");
        assert!(err.to_string().contains("128x128"), "{err}");
    }
}
