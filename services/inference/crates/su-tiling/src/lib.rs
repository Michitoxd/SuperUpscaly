//! # su-tiling
//!
//! Planificacion de tiles, presupuesto de VRAM y composicion sin costuras.
//!
//! Es el crate que resuelve el defecto mas citado de las herramientas de
//! upscaling: el desbordamiento de memoria de la GPU. Dos ideas lo sostienen:
//!
//! - **El tile se elige, no se adivina.** Se estima el pico con la calibracion
//!   medida por `(modelo, EP, dispositivo, tile)` y se toma el mayor que cabe.
//! - **Nunca se falla en silencio.** Si la estimacion se queda corta, la escalera
//!   de degradacion baja el tile, despues libera el modelo y por ultimo cae a CPU.
//!
//! No depende de ONNX Runtime ni del sistema de archivos: es aritmetica pura,
//! y por eso esta cubierto con tests exhaustivos.

pub mod calibration;
pub mod plan;
pub mod vram;

pub use calibration::{
    CalibrationEntry, CalibrationKey, CalibrationSample, CalibrationStore,
};
pub use plan::{
    axis_weights, composite, coverage, Tile, TilePlan, MAX_TILES, MIN_TILE,
};
pub use vram::{
    choose_tile, degrade_tile, overlap_for_tile, overlap_for_tile_boosted, ProviderKind,
    VramBudget, DEFAULT_CANDIDATES, MAX_DEGRADATION_STEPS, MAX_OVERLAP, MIN_OVERLAP,
};
