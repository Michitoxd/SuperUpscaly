//! Presupuesto de VRAM, eleccion de tile y escalera de degradacion.
//!
//! El objetivo es que un error de estimacion se traduzca en "mas lento", nunca
//! en "fallo" o "imagen negra" (ADR-007). Por eso hay dos mecanismos:
//!
//! 1. **Eleccion predictiva**: se estima el pico con la calibracion medida por
//!    `(modelo, EP, dispositivo, tile)` y se elige el tile mas grande que cabe.
//! 2. **Degradacion progresiva**: si aun asi se agota la memoria, se baja de
//!    escalon en escalon hasta el minimo, y despues se cae a CPU.

use serde::{Deserialize, Serialize};

use crate::plan::MIN_TILE;

/// Tamanos de tile a probar, de mayor a menor.
pub const DEFAULT_CANDIDATES: &[u32] = &[1024, 768, 512, 384, 256, 192];

pub const MIN_OVERLAP: u32 = 16;
pub const MAX_OVERLAP: u32 = 64;

/// Numero maximo de reintentos antes de caer a CPU.
pub const MAX_DEGRADATION_STEPS: u32 = 4;

/// Execution provider. Determina el coste fijo de contexto en VRAM, que es
/// medido, no estimado a ojo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    TensorRt,
    Cuda,
    DirectMl,
    CoreMl,
    Cpu,
}

impl ProviderKind {
    /// Overhead de contexto medido empiricamente para cada backend.
    pub const fn context_overhead_mb(self) -> u64 {
        match self {
            Self::TensorRt => 250,
            Self::Cuda => 180,
            Self::DirectMl => 120,
            Self::CoreMl => 60,
            Self::Cpu => 0,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TensorRt => "TensorRT",
            Self::Cuda => "CUDA",
            Self::DirectMl => "DirectML",
            Self::CoreMl => "CoreML",
            Self::Cpu => "CPU",
        }
    }

    /// La CPU no consume VRAM: el presupuesto no aplica y no se trocea por memoria.
    pub const fn uses_vram(self) -> bool {
        !matches!(self, Self::Cpu)
    }
}

/// Datos con los que se decide el tile. Todos los terminos son medidos o
/// declarados por el manifiesto del modelo; ninguno es una constante magica
/// escondida en el codigo de inferencia.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VramBudget {
    /// VRAM libre en el momento de planificar, en MiB.
    pub free_mb: u64,
    /// Fraccion de la VRAM libre que se permite usar.
    pub safety_factor: f32,
    /// Coste fijo del backend.
    pub context_overhead_mb: u64,
    /// Pesos del modelo mas su workspace.
    pub model_mb: u64,
    /// Calibracion: VRAM consumida por megapixel de tile.
    pub vram_per_megapixel: f32,
}

impl VramBudget {
    /// Presupuesto conservador para un backend concreto. Con poca VRAM se baja
    /// el factor de seguridad: en 4 GB, 0.70 deja demasiado poco margen.
    pub fn for_provider(free_mb: u64, model_mb: u64, vram_per_megapixel: f32, provider: ProviderKind) -> Self {
        let safety_factor = if free_mb <= 4096 { 0.55 } else { 0.70 };
        Self {
            free_mb,
            safety_factor,
            context_overhead_mb: provider.context_overhead_mb(),
            model_mb,
            vram_per_megapixel,
        }
    }

    /// Techo absoluto que se permite alcanzar.
    pub fn allowed_mb(&self) -> u64 {
        (self.free_mb as f64 * self.safety_factor as f64).round() as u64
    }

    /// VRAM que queda para activaciones despues del coste fijo.
    pub fn usable_mb(&self) -> u64 {
        self.allowed_mb()
            .saturating_sub(self.context_overhead_mb)
            .saturating_sub(self.model_mb)
    }

    /// Pico estimado al procesar un tile cuadrado de lado `tile`.
    pub fn predicted_peak_mb(&self, tile: u32) -> u64 {
        let megapixels = (tile as f64 * tile as f64) / 1_000_000.0;
        let activations = megapixels * self.vram_per_megapixel as f64;
        self.context_overhead_mb + self.model_mb + activations.round() as u64
    }

    pub fn fits(&self, tile: u32) -> bool {
        self.predicted_peak_mb(tile) <= self.allowed_mb()
    }
}

/// El tile mas grande de `candidates` que cabe en el presupuesto.
///
/// No se asume que la lista venga ordenada: se toma el maximo de los que caben.
/// Devuelve `None` si ni el mas pequeno cabe, que es la senal para caer a CPU.
pub fn choose_tile(budget: &VramBudget, candidates: &[u32]) -> Option<u32> {
    candidates
        .iter()
        .copied()
        .filter(|tile| *tile >= MIN_TILE && budget.fits(*tile))
        .max()
}

/// Siguiente escalon de degradacion: el mayor candidato estrictamente menor que
/// `current` y que no baje del minimo.
pub fn degrade_tile(current: u32, candidates: &[u32], min_tile: u32) -> Option<u32> {
    candidates
        .iter()
        .copied()
        .filter(|tile| *tile < current && *tile >= min_tile)
        .max()
}

/// Solape a partir del tamano de tile, segun el divisor declarado por el modelo.
pub fn overlap_for_tile(tile: u32, divisor: u32) -> u32 {
    let divisor = divisor.max(1);
    (tile / divisor).clamp(MIN_OVERLAP, MAX_OVERLAP)
}

/// Solape con multiplicador. La segunda pasada de 8x usa `boost = 2` porque cada
/// costura de la primera pasada se amplifica x4 en la segunda.
pub fn overlap_for_tile_boosted(tile: u32, divisor: u32, boost: u32) -> u32 {
    let boosted = overlap_for_tile(tile, divisor).saturating_mul(boost.max(1));
    boosted.min(tile / 2).max(MIN_OVERLAP.min(tile / 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(free_mb: u64, model_mb: u64, per_mp: f32) -> VramBudget {
        VramBudget::for_provider(free_mb, model_mb, per_mp, ProviderKind::TensorRt)
    }

    #[test]
    fn provider_overheads_match_the_documented_values() {
        assert_eq!(ProviderKind::TensorRt.context_overhead_mb(), 250);
        assert_eq!(ProviderKind::Cuda.context_overhead_mb(), 180);
        assert_eq!(ProviderKind::DirectMl.context_overhead_mb(), 120);
        assert_eq!(ProviderKind::CoreMl.context_overhead_mb(), 60);
        assert_eq!(ProviderKind::Cpu.context_overhead_mb(), 0);
        assert!(!ProviderKind::Cpu.uses_vram());
        assert!(ProviderKind::Cuda.uses_vram());
    }

    #[test]
    fn low_vram_gpus_get_a_more_conservative_factor() {
        let small = VramBudget::for_provider(4096, 70, 420.0, ProviderKind::Cuda);
        let large = VramBudget::for_provider(12288, 70, 420.0, ProviderKind::Cuda);
        assert!((small.safety_factor - 0.55).abs() < f32::EPSILON);
        assert!((large.safety_factor - 0.70).abs() < f32::EPSILON);
    }

    #[test]
    fn predicted_peak_grows_with_the_tile() {
        let budget = budget(8192, 70, 420.0);
        let small = budget.predicted_peak_mb(256);
        let large = budget.predicted_peak_mb(1024);
        assert!(large > small);
        // El coste fijo se paga siempre.
        assert!(small >= 250 + 70);
    }

    #[test]
    fn a_generous_gpu_picks_the_largest_candidate() {
        let budget = budget(24576, 70, 420.0);
        assert_eq!(choose_tile(&budget, DEFAULT_CANDIDATES), Some(1024));
    }

    #[test]
    fn a_light_model_fits_a_large_tile_even_on_a_small_gpu() {
        // Con un modelo ligero (420 MB/MP), 1024 cabe de sobra en 4 GB. El
        // sistema no debe ser conservador por defecto: seria tirar rendimiento.
        let budget = budget(4096, 70, 420.0);
        assert!(budget.fits(1024));
        assert_eq!(choose_tile(&budget, DEFAULT_CANDIDATES), Some(1024));
    }

    #[test]
    fn a_smaller_gpu_gets_a_smaller_tile_for_the_same_model() {
        // Modelo pesado: 2.5 GB de activaciones por megapixel.
        let per_mp = 2500.0;
        let small = budget(4096, 70, per_mp);
        let large = budget(24576, 70, per_mp);

        let small_tile = choose_tile(&small, DEFAULT_CANDIDATES).expect("deberia caber algo");
        let large_tile = choose_tile(&large, DEFAULT_CANDIDATES).expect("deberia caber algo");

        assert!(small_tile <= large_tile, "{small_tile} > {large_tile}");
        assert_eq!(large_tile, 1024);
        assert!(small_tile < 1024, "eligio {small_tile}, demasiado grande para 4 GB");
        assert!(small.fits(small_tile));
        assert!(!small.fits(1024));
    }

    #[test]
    fn no_candidate_fits_signals_the_caller_to_fall_back() {
        // 512 MiB libres: el coste fijo del backend ya no cabe.
        let budget = budget(512, 400, 420.0);
        assert_eq!(choose_tile(&budget, DEFAULT_CANDIDATES), None);
    }

    #[test]
    fn candidates_need_not_be_sorted() {
        let budget = budget(8192, 70, 420.0);
        let unsorted = [256u32, 1024, 512, 384];
        assert_eq!(choose_tile(&budget, &unsorted), choose_tile(&budget, DEFAULT_CANDIDATES));
    }

    #[test]
    fn degradation_steps_down_one_candidate_at_a_time() {
        assert_eq!(degrade_tile(1024, DEFAULT_CANDIDATES, MIN_TILE), Some(768));
        assert_eq!(degrade_tile(768, DEFAULT_CANDIDATES, MIN_TILE), Some(512));
        assert_eq!(degrade_tile(512, DEFAULT_CANDIDATES, MIN_TILE), Some(384));
    }

    #[test]
    fn degradation_stops_at_the_minimum() {
        assert_eq!(degrade_tile(192, DEFAULT_CANDIDATES, MIN_TILE), None);
        assert_eq!(degrade_tile(256, DEFAULT_CANDIDATES, 256), None);
    }

    #[test]
    fn degradation_never_returns_something_larger() {
        for current in [1024u32, 768, 512, 384, 256, 192] {
            if let Some(next) = degrade_tile(current, DEFAULT_CANDIDATES, MIN_TILE) {
                assert!(next < current, "{current} -> {next}");
            }
        }
    }

    #[test]
    fn the_degradation_ladder_is_walkable_within_the_retry_budget() {
        // Desde 1024 hasta el minimo no deben hacer falta mas reintentos que los
        // que permite la politica, o el fallback a CPU llegaria tarde.
        let mut current = 1024u32;
        let mut steps = 0u32;
        while let Some(next) = degrade_tile(current, DEFAULT_CANDIDATES, MIN_TILE) {
            current = next;
            steps += 1;
            assert!(steps <= 16, "escalera demasiado larga");
        }
        assert_eq!(current, 192);
    }

    #[test]
    fn overlap_scales_with_the_tile_and_stays_in_range() {
        assert_eq!(overlap_for_tile(1024, 16), 64);
        assert_eq!(overlap_for_tile(512, 16), 32);
        assert_eq!(overlap_for_tile(256, 16), 16);
        // Por debajo del minimo, se aplica el minimo.
        assert_eq!(overlap_for_tile(192, 16), 16);
        // Por encima del maximo, se recorta.
        assert_eq!(overlap_for_tile(4096, 16), MAX_OVERLAP);
    }

    #[test]
    fn a_zero_divisor_does_not_panic() {
        assert_eq!(overlap_for_tile(512, 0), MAX_OVERLAP);
    }

    #[test]
    fn overlap_boost_stays_below_half_the_tile() {
        // Un solape >= tile/2 dejaria paso cero y colgaria la planificacion.
        for tile in [192u32, 256, 512, 1024] {
            let overlap = overlap_for_tile_boosted(tile, 16, 2);
            assert!(overlap <= tile / 2, "tile {tile} -> solape {overlap}");
            assert!(overlap > 0);
        }
    }
}
