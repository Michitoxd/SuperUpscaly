//! Tipos de dominio. Los nombres de campo en JSON son `camelCase` para coincidir
//! con los contratos de TypeScript (`packages/shared/src/types.ts`).

use serde::{Deserialize, Serialize};

use crate::error::{SuError, SuResult};

/// Los dos unicos modos del producto. No hay mas categorias por diseno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Photo,
    Illustration,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Photo => "photo",
            Self::Illustration => "illustration",
        }
    }
}

/// Factores de escala soportados. Se modela como enum para que un `scale` de 3
/// sea imposible de construir, no solo improbable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub enum Scale {
    X2,
    X4,
    X8,
}

impl Scale {
    pub const fn factor(self) -> u32 {
        match self {
            Self::X2 => 2,
            Self::X4 => 4,
            Self::X8 => 8,
        }
    }
}

impl TryFrom<u32> for Scale {
    type Error = SuError;

    fn try_from(value: u32) -> SuResult<Self> {
        match value {
            2 => Ok(Self::X2),
            4 => Ok(Self::X4),
            8 => Ok(Self::X8),
            other => Err(SuError::Internal(format!(
                "scale {other} is not supported (allowed: 2, 4, 8)"
            ))),
        }
    }
}

impl From<Scale> for u32 {
    fn from(value: Scale) -> Self {
        value.factor()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Png,
    Jpg,
    Webp,
}

impl OutputFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
        }
    }

    /// PNG es sin perdida: la calidad no aplica.
    pub const fn is_lossy(self) -> bool {
        !matches!(self, Self::Png)
    }
}

/// Etapas del pipeline. El orden de las variantes es el orden de ejecucion
/// habitual, pero quien manda es la lista de etapas del pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageId {
    Decode,
    Analyze,
    Denoise,
    Upscale,
    Face,
    Sharpen,
    Encode,
}

impl StageId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Analyze => "analyze",
            Self::Denoise => "denoise",
            Self::Upscale => "upscale",
            Self::Face => "face",
            Self::Sharpen => "sharpen",
            Self::Encode => "encode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Completed,
    /// Termino con exitos y fallos mezclados.
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Pending,
    Running,
    Done,
    /// Completado tras una degradacion (tile menor, fallback a CPU).
    Degraded,
    Failed,
    Skipped,
}

/// Tipo de contenido detectado por el analisis previo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Photo,
    Illustration,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceChoice {
    Auto,
    Cpu,
    Gpu,
}

/// Seleccion de tile. `Auto` deja que decida el presupuesto de VRAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TileChoice {
    Auto,
    #[serde(rename = "256")]
    Px256,
    #[serde(rename = "384")]
    Px384,
    #[serde(rename = "512")]
    Px512,
    #[serde(rename = "768")]
    Px768,
    #[serde(rename = "1024")]
    Px1024,
}

impl TileChoice {
    pub const fn explicit(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Px256 => Some(256),
            Self::Px384 => Some(384),
            Self::Px512 => Some(512),
            Self::Px768 => Some(768),
            Self::Px1024 => Some(1024),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DenoiseChoice {
    Off,
    Auto,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaceRestoreChoice {
    Off,
    Auto,
    Low,
    Medium,
    High,
}

impl FaceRestoreChoice {
    /// Intensidad 0..1 que se pasa a la etapa de restauracion facial.
    pub const fn intensity(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Auto | Self::Medium => 0.85,
            Self::Low => 0.60,
            Self::High => 1.0,
        }
    }

    pub const fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelChainMode {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSettings {
    pub dir: String,
    pub format: OutputFormat,
    /// 1..100. Solo se aplica a formatos con perdida.
    pub quality: u8,
    #[serde(default)]
    pub suffix: String,
    #[serde(default = "default_true")]
    pub preserve_metadata: bool,
    /// Empaquetar el resultado del lote en un ZIP.
    #[serde(default)]
    pub zip_output: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOptions {
    pub tile_size: TileChoice,
    pub device: DeviceChoice,
    /// Imagenes simultaneas. 1 por GPU es el optimo medido (ver ADR-014).
    pub concurrency: u8,
    pub unload_between_images: bool,
    pub model_chain_mode: ModelChainMode,
    pub upscale_model: Option<String>,
    pub face_restore: FaceRestoreChoice,
    pub denoise: DenoiseChoice,
    pub sharpen: bool,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            tile_size: TileChoice::Auto,
            device: DeviceChoice::Auto,
            concurrency: 1,
            unload_between_images: false,
            model_chain_mode: ModelChainMode::Auto,
            upscale_model: None,
            face_restore: FaceRestoreChoice::Auto,
            denoise: DenoiseChoice::Auto,
            sharpen: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub mode: Mode,
    pub scale: Scale,
    pub items: Vec<String>,
    pub output: OutputSettings,
    #[serde(default)]
    pub options: JobOptions,
    #[serde(default)]
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemProgress {
    pub status: ItemStatus,
    /// 0.0..1.0
    pub progress: f32,
    pub stage: Option<StageId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItem {
    pub id: String,
    pub src_path: String,
    pub out_path: Option<String>,
    pub status: ItemStatus,
    pub attempt: u8,
    pub progress: f32,
    pub stage: Option<StageId>,
    pub duration_ms: Option<u64>,
    pub vram_peak_mb: Option<u64>,
    /// Etapas realmente ejecutadas (las condiciones pueden omitir algunas).
    #[serde(default)]
    pub effective_pipeline: Vec<StageId>,
    /// Etapas que no se ejecutaron y por que, en formato `"etapa: motivo"`.
    ///
    /// `effective_pipeline` dice **que** no se hizo; esto dice **por que**. Sin
    /// el motivo, un trabajo que reanudo desde SQLite solo puede ensenar una
    /// lista de etapas mas corta y el usuario no tiene forma de saber si falta
    /// algo por un limite de tamano, por una preferencia suya o por un fallo.
    /// Es `#[serde(default)]` porque las filas guardadas antes de que existiera
    /// este campo tienen que seguir cargando.
    #[serde(default)]
    pub skipped: Vec<String>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub degraded: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub created_at: String,
    pub status: JobStatus,
    pub mode: Mode,
    pub scale: Scale,
    pub pipeline_id: String,
    pub output: OutputSettings,
    pub options: JobOptions,
    pub progress: JobProgress,
    pub items: Vec<JobItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_rejects_unsupported_factors() {
        assert_eq!(Scale::try_from(2).unwrap(), Scale::X2);
        assert_eq!(Scale::try_from(4).unwrap(), Scale::X4);
        assert_eq!(Scale::try_from(8).unwrap(), Scale::X8);
        assert!(Scale::try_from(3).is_err());
        assert!(Scale::try_from(0).is_err());
    }

    #[test]
    fn scale_serialises_as_a_number() {
        let json = serde_json::to_string(&Scale::X4).unwrap();
        assert_eq!(json, "4");
        let parsed: Scale = serde_json::from_str("8").unwrap();
        assert_eq!(parsed, Scale::X8);
    }

    #[test]
    fn deserialising_an_invalid_scale_fails() {
        let result: Result<Scale, _> = serde_json::from_str("3");
        assert!(result.is_err());
    }

    #[test]
    fn tile_choice_round_trips_through_json() {
        let json = serde_json::to_string(&TileChoice::Px512).unwrap();
        assert_eq!(json, "\"512\"");
        let parsed: TileChoice = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(parsed, TileChoice::Auto);
        assert_eq!(TileChoice::Px768.explicit(), Some(768));
        assert_eq!(TileChoice::Auto.explicit(), None);
    }

    #[test]
    fn job_request_parses_a_full_payload() {
        let payload = r#"{
            "mode": "photo",
            "scale": 4,
            "items": ["/tmp/a.png"],
            "output": { "dir": "/tmp/out", "format": "png", "quality": 95 },
            "options": { "tileSize": "auto", "device": "auto", "concurrency": 1,
                         "unloadBetweenImages": false, "modelChainMode": "auto",
                         "faceRestore": "auto", "denoise": "auto", "sharpen": false }
        }"#;

        let request: JobRequest = serde_json::from_str(payload).unwrap();
        assert_eq!(request.mode, Mode::Photo);
        assert_eq!(request.scale, Scale::X4);
        assert_eq!(request.items.len(), 1);
        // `suffix` y `preserveMetadata` tienen valor por defecto.
        assert_eq!(request.output.suffix, "");
        assert!(request.output.preserve_metadata);
        assert_eq!(request.options.concurrency, 1);
    }
}
