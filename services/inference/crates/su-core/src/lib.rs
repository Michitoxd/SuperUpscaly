//! # su-core
//!
//! Dominio de SuperUpscaly: tipos, taxonomia de errores y motor de pipelines.
//!
//! Este crate **no tiene dependencias de inferencia ni de sistema de archivos**.
//! Es logica pura y testeable, lo que permite cubrir con tests unitarios las
//! partes donde de verdad se cometen errores: la seleccion de etapas, la
//! validacion de condiciones y la aritmetica de escalas.

pub mod analysis;
pub mod error;
pub mod pipeline;
pub mod types;

pub use analysis::{Analysis, FaceBox};
pub use error::{ErrorCode, SuError, SuResult};
pub use pipeline::{
    CmpOp, Condition, EvalVars, Pipeline, PipelineSet, Scalar, Stage, StageOp,
};
pub use types::{
    ContentKind, DenoiseChoice, DeviceChoice, FaceRestoreChoice, ItemStatus, Job, JobItem,
    JobOptions, JobProgress, JobRequest, JobStatus, Mode, ModelChainMode, OutputFormat,
    OutputSettings, Scale, StageId, TileChoice,
};
