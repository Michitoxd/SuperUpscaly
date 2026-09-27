//! Calibracion de VRAM y tiempo por `(modelo, EP, dispositivo, tile)`.
//!
//! Los valores del manifiesto son estimaciones escritas a mano. La primera vez
//! que un modelo se ejecuta en un equipo se mide el consumo real y se guarda
//! aqui; a partir de ahi la eleccion de tile deja de ser una conjetura.
//!
//! ## Por que se suaviza en lugar de reemplazar
//!
//! Una medicion aislada no es fiable: otra aplicacion puede estar usando la GPU,
//! o el driver puede estar reorganizando memoria en ese momento. Si se
//! reemplazara el valor con cada muestra, un pico puntual fijaria un tile
//! demasiado conservador para siempre. Se usa una media exponencial con un
//! factor que crece con el numero de muestras: las primeras pesan mucho y las
//! siguientes cada vez menos, asi que el valor converge en lugar de oscilar.
//!
//! ## Por que se rechazan los valores absurdos
//!
//! Un medidor de VRAM puede devolver un pico disparatado si algo mas esta
//! consumiendo memoria. Aceptar esa muestra envenenaria la calibracion de ese
//! modelo para todas las sesiones siguientes. Una muestra que se aleja mas de
//! 4x del valor actual se descarta: es mas probable que sea ruido del entorno
//! que un cambio real de consumo.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use su_core::{SuError, SuResult};

use crate::vram::{ProviderKind, VramBudget};

/// Muestras que hacen falta para que el valor converja.
const CONVERGENCE_SAMPLES: u32 = 8;

/// Desviacion maxima aceptada respecto al valor actual antes de descartar una
/// muestra. Un consumo que se multiplica por cuatro de una vez no es un cambio
/// de comportamiento del modelo, es otra cosa usando la GPU.
const OUTLIER_FACTOR: f32 = 4.0;

/// Identifica una medicion. Es la clave con la que se guarda y se busca.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationKey {
    pub model_id: String,
    pub provider: String,
    pub device_id: u32,
    pub tile: u32,
}

impl CalibrationKey {
    pub fn new(
        model_id: impl Into<String>,
        provider: ProviderKind,
        device_id: u32,
        tile: u32,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            provider: provider.as_str().to_string(),
            device_id,
            tile,
        }
    }

    /// Clave plana, con el mismo formato que el ejemplo de
    /// `docs/02-arquitectura.md`: `4x-ultrasharp|TensorRT|0|512`.
    pub fn to_key(&self) -> String {
        format!("{}|{}|{}|{}", self.model_id, self.provider, self.device_id, self.tile)
    }

    pub fn parse(key: &str) -> Option<Self> {
        let mut parts = key.split('|');
        let model_id = parts.next()?.to_string();
        let provider = parts.next()?.to_string();
        let device_id = parts.next()?.parse().ok()?;
        let tile = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            model_id,
            provider,
            device_id,
            tile,
        })
    }
}

/// Medicion de una ejecucion concreta.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSample {
    /// Pico de VRAM observado durante la inferencia, en MiB.
    pub vram_peak_mb: u64,
    /// Tiempo de la inferencia, en milisegundos.
    pub elapsed_ms: u64,
}

/// Valor calibrado y consolidado.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationEntry {
    /// VRAM por megapixel de tile.
    pub vram_per_megapixel: f32,
    /// Tiempo por megapixel de tile.
    pub ms_per_megapixel: f32,
    pub samples: u32,
    /// Marca de tiempo UNIX de la ultima actualizacion.
    pub updated_at: u64,
}

impl CalibrationEntry {
    /// Primera medicion: se acepta tal cual, sin suavizar.
    fn from_sample(sample: CalibrationSample, megapixels: f32, now: u64) -> Self {
        let megapixels = megapixels.max(0.01);
        Self {
            vram_per_megapixel: sample.vram_peak_mb as f32 / megapixels,
            ms_per_megapixel: sample.elapsed_ms as f32 / megapixels,
            samples: 1,
            updated_at: now,
        }
    }

    fn absorb(&mut self, sample: CalibrationSample, megapixels: f32, now: u64) {
        let megapixels = megapixels.max(0.01);
        let observed_vram = sample.vram_peak_mb as f32 / megapixels;
        let observed_time = sample.elapsed_ms as f32 / megapixels;

        if !is_plausible(self.vram_per_megapixel, observed_vram) {
            return;
        }

        // Peso decreciente: las primeras muestras corrigen mucho, las siguientes
        // afinan. Sin esto, el valor oscilaria con cada ejecucion.
        let weight = 1.0 / (self.samples.min(CONVERGENCE_SAMPLES) as f32 + 1.0);

        self.vram_per_megapixel =
            self.vram_per_megapixel * (1.0 - weight) + observed_vram * weight;
        self.ms_per_megapixel = self.ms_per_megapixel * (1.0 - weight) + observed_time * weight;
        self.samples = self.samples.saturating_add(1);
        self.updated_at = now;
    }
}

fn is_plausible(current: f32, observed: f32) -> bool {
    if !observed.is_finite() || observed <= 0.0 {
        return false;
    }
    let ratio = observed / current.max(f32::MIN_POSITIVE);
    (1.0 / OUTLIER_FACTOR..=OUTLIER_FACTOR).contains(&ratio)
}

/// Conjunto de calibraciones, serializable a `cache/calibration.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CalibrationStore {
    entries: HashMap<String, CalibrationEntry>,
}

impl CalibrationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &CalibrationKey) -> Option<&CalibrationEntry> {
        self.entries.get(&key.to_key())
    }

    /// Incorpora una medicion. `megapixels` es el area del tile en millones de
    /// pixeles, que es lo que hace comparables modelos de distinto tamano.
    ///
    /// La primera medicion de una clave se guarda tal cual; las siguientes se
    /// suavizan sobre la anterior.
    pub fn record(
        &mut self,
        key: &CalibrationKey,
        sample: CalibrationSample,
        megapixels: f32,
        now: u64,
    ) -> &CalibrationEntry {
        let flat = key.to_key();

        match self.entries.get_mut(&flat) {
            Some(entry) => entry.absorb(sample, megapixels, now),
            None => {
                self.entries.insert(
                    flat.clone(),
                    CalibrationEntry::from_sample(sample, megapixels, now),
                );
            }
        }

        self.entries
            .get(&flat)
            .expect("la entrada se acaba de insertar o ya existia")
    }

    /// Aplica la calibracion a un presupuesto, si hay medicion para esa clave.
    ///
    /// Devuelve el presupuesto sin tocar cuando no hay datos: es mejor usar la
    /// estimacion del manifiesto que inventar un valor.
    pub fn apply(&self, key: &CalibrationKey, mut budget: VramBudget) -> VramBudget {
        if let Some(entry) = self.get(key) {
            budget.vram_per_megapixel = entry.vram_per_megapixel;
        }
        budget
    }

    /// Tiempo estimado por megapixel, para avisar del tiempo de un lote antes de
    /// empezarlo. `None` si ese modelo aun no se ha medido.
    pub fn ms_per_megapixel(&self, key: &CalibrationKey) -> Option<f32> {
        self.get(key).map(|entry| entry.ms_per_megapixel)
    }

    pub fn from_json(raw: &str) -> SuResult<Self> {
        serde_json::from_str(raw)
            .map_err(|error| SuError::Internal(format!("calibracion no valida: {error}")))
    }

    pub fn to_json(&self) -> SuResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| SuError::Internal(format!("calibracion no serializable: {error}")))
    }

    /// Descarta las entradas de un modelo concreto. Se usa cuando se reinstala o
    /// se cambia la version de un modelo: su consumo anterior ya no aplica.
    pub fn forget_model(&mut self, model_id: &str) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|key, _| !key.starts_with(&format!("{model_id}|")));
        before - self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(tile: u32) -> CalibrationKey {
        CalibrationKey::new("4x-ultrasharp", ProviderKind::TensorRt, 0, tile)
    }

    fn sample(vram_peak_mb: u64, elapsed_ms: u64) -> CalibrationSample {
        CalibrationSample {
            vram_peak_mb,
            elapsed_ms,
        }
    }

    #[test]
    fn the_key_round_trips() {
        let original = key(512);
        let flat = original.to_key();
        assert_eq!(flat, "4x-ultrasharp|TensorRT|0|512");
        assert_eq!(CalibrationKey::parse(&flat), Some(original));
    }

    #[test]
    fn malformed_keys_are_rejected() {
        assert_eq!(CalibrationKey::parse(""), None);
        assert_eq!(CalibrationKey::parse("solo|dos"), None);
        assert_eq!(CalibrationKey::parse("a|b|c|d|e"), None);
        assert_eq!(CalibrationKey::parse("modelo|CPU|no-numero|512"), None);
    }

    #[test]
    fn the_first_sample_is_taken_as_is() {
        let mut store = CalibrationStore::new();
        // Un tile de 512x512 son 0.262 MP; 400 MiB de pico -> ~1526 MiB/MP.
        let entry = store.record(&key(512), sample(400, 800), 0.262, 1000);

        assert_eq!(entry.samples, 1);
        assert!((entry.vram_per_megapixel - 1526.7).abs() < 1.0, "{}", entry.vram_per_megapixel);
        assert!((entry.ms_per_megapixel - 3053.4).abs() < 5.0);
    }

    #[test]
    fn subsequent_samples_converge_instead_of_jumping() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(400, 800), 0.262, 1000);

        // Una segunda medicion algo mayor mueve el valor, pero no lo reemplaza.
        let entry = store.record(&key(512), sample(500, 900), 0.262, 2000);
        let after_second = entry.vram_per_megapixel;

        assert!(after_second > 1526.0, "deberia subir");
        assert!(after_second < 1900.0, "no deberia saltar al valor nuevo: {after_second}");
        assert_eq!(entry.samples, 2);

        // Muchas mediciones en el mismo valor deben acercarse a el.
        for tick in 0..40 {
            store.record(&key(512), sample(500, 900), 0.262, 3000 + tick);
        }
        let settled = store.get(&key(512)).unwrap().vram_per_megapixel;
        assert!(
            (settled - 1908.4).abs() < 30.0,
            "deberia converger a ~1908, converge a {settled}"
        );
    }

    #[test]
    fn an_absurd_sample_is_discarded() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(400, 800), 0.262, 1000);
        let before = store.get(&key(512)).unwrap().vram_per_megapixel;

        // Otra aplicacion se comio la GPU: el medidor devuelve un pico 20x mayor.
        let entry = store.record(&key(512), sample(8000, 900), 0.262, 2000);

        assert_eq!(entry.samples, 1, "la muestra no deberia contarse");
        assert_eq!(entry.vram_per_megapixel, before, "el valor no deberia moverse");
    }

    #[test]
    fn a_zero_reading_is_discarded() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(400, 800), 0.262, 1000);
        let entry = store.record(&key(512), sample(0, 0), 0.262, 2000);
        assert_eq!(entry.samples, 1);
    }

    #[test]
    fn different_tiles_are_tracked_separately() {
        let mut store = CalibrationStore::new();
        store.record(&key(256), sample(100, 200), 0.0655, 1000);
        store.record(&key(1024), sample(1600, 3200), 1.048, 1000);

        assert_eq!(store.len(), 2);
        assert!(store.get(&key(256)).unwrap().vram_per_megapixel < 1600.0);
        assert!(store.get(&key(1024)).unwrap().vram_per_megapixel > 1500.0);
    }

    #[test]
    fn different_providers_are_tracked_separately() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(400, 800), 0.262, 1000);
        store.record(
            &CalibrationKey::new("4x-ultrasharp", ProviderKind::Cpu, 0, 512),
            sample(0, 4000),
            0.262,
            1000,
        );
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn the_store_round_trips_through_json() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(400, 800), 0.262, 1000);
        store.record(&key(1024), sample(1600, 3200), 1.048, 1000);

        let json = store.to_json().expect("serializacion");
        let restored = CalibrationStore::from_json(&json).expect("deserializacion");
        assert_eq!(restored, store);
    }

    #[test]
    fn corrupt_json_is_rejected_clearly() {
        let error = CalibrationStore::from_json("{ esto no es json }").unwrap_err();
        assert!(error.to_string().contains("calibracion"), "{error}");
    }

    #[test]
    fn an_empty_store_serialises_to_an_empty_object() {
        let json = CalibrationStore::new().to_json().expect("serializacion");
        assert_eq!(json.trim(), "{}");
        assert!(CalibrationStore::from_json(&json).unwrap().is_empty());
    }

    #[test]
    fn the_calibration_overrides_the_manifest_estimate() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(200, 800), 0.262, 1000);

        let budget = VramBudget::for_provider(8192, 70, 420.0, ProviderKind::TensorRt);
        assert!((budget.vram_per_megapixel - 420.0).abs() < f32::EPSILON);

        let calibrated = store.apply(&key(512), budget);
        assert!(
            (calibrated.vram_per_megapixel - 763.4).abs() < 1.0,
            "{}",
            calibrated.vram_per_megapixel
        );
    }

    #[test]
    fn without_data_the_budget_is_untouched() {
        let store = CalibrationStore::new();
        let budget = VramBudget::for_provider(8192, 70, 420.0, ProviderKind::TensorRt);
        let same = store.apply(&key(512), budget);
        assert_eq!(same, budget);
    }

    #[test]
    fn the_measured_time_is_available_for_estimates() {
        let mut store = CalibrationStore::new();
        assert_eq!(store.ms_per_megapixel(&key(512)), None);

        store.record(&key(512), sample(400, 800), 0.262, 1000);
        let ms = store.ms_per_megapixel(&key(512)).expect("deberia haber medida");
        assert!(ms > 3000.0 && ms < 3100.0, "{ms}");
    }

    #[test]
    fn forgetting_a_model_removes_only_its_entries() {
        let mut store = CalibrationStore::new();
        store.record(&key(256), sample(100, 200), 0.0655, 1000);
        store.record(&key(512), sample(400, 800), 0.262, 1000);
        store.record(
            &CalibrationKey::new("anime-6b", ProviderKind::TensorRt, 0, 512),
            sample(200, 400),
            0.262,
            1000,
        );
        assert_eq!(store.len(), 3);

        let removed = store.forget_model("4x-ultrasharp");
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 1);
        assert!(store.get(&key(512)).is_none());
    }

    #[test]
    fn forgetting_an_unknown_model_is_harmless() {
        let mut store = CalibrationStore::new();
        store.record(&key(512), sample(400, 800), 0.262, 1000);
        assert_eq!(store.forget_model("no-existe"), 0);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn a_tiny_tile_does_not_divide_by_zero() {
        // Un area de cero no deberia producir infinitos ni NaN.
        let mut store = CalibrationStore::new();
        let entry = store.record(&key(64), sample(10, 20), 0.0, 1000);
        assert!(entry.vram_per_megapixel.is_finite());
        assert!(entry.ms_per_megapixel.is_finite());
    }
}
