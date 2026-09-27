//! # su-models
//!
//! Manifiesto de modelos, verificacion por hash y estado del cache local.
//!
//! Dos decisiones que importan:
//!
//! - **La descarga no vive aqui.** En la Fase 2 el manifiesto apunta a rutas
//!   locales, como pide el plan ("puedes simularlo con una carpeta local por
//!   ahora"). La descarga real entra en la Fase 4 reutilizando la verificacion
//!   que ya esta escrita aqui.
//! - **Un hash ausente no es un hash valido.** Un modelo sin `sha256` se reporta
//!   como `Unverified` y se avisa; nunca se trata como verificado. Silenciar eso
//!   seria exactamente el tipo de fallo silencioso que el proyecto quiere evitar.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use su_core::{SuError, SuResult};

/// Tipos de modelo. Determina en que etapa del pipeline puede usarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    /// Modelo base para fotografias.
    Photo,
    /// Modelo base para dibujo y anime.
    Illustration,
    /// Reduccion de ruido previa.
    Denoise,
    /// Restauracion facial.
    Face,
    /// Deteccion de rostros (analisis).
    Detector,
    /// Clasificador foto/ilustracion (analisis).
    Classifier,
}

impl ModelKind {
    /// Si es un modelo base, con que modo del producto se corresponde.
    pub const fn base_mode(self) -> Option<&'static str> {
        match self {
            Self::Photo => Some("photo"),
            Self::Illustration => Some("illustration"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub commercial_use: bool,
    /// Aviso que se muestra antes de descargar, p. ej. restricciones de uso.
    #[serde(default)]
    pub note: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Precision {
    /// Si el modelo tolera `fp16` sin degradarse.
    #[serde(default = "default_true")]
    pub fp16_safe: bool,
    #[serde(default = "default_precision")]
    pub preferred: String,
}

fn default_precision() -> String {
    "fp16".to_string()
}

impl Default for Precision {
    fn default() -> Self {
        Self {
            fp16_safe: true,
            preferred: default_precision(),
        }
    }
}

/// Pistas de tiling declaradas por el modelo. Los valores de VRAM y tiempo son
/// estimaciones iniciales: la primera ejecucion los recalibra y los guarda en
/// `cache/calibration.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TilingHints {
    #[serde(default = "default_candidates")]
    pub candidates: Vec<u32>,
    #[serde(default = "default_overlap_divisor")]
    pub overlap_divisor: u32,
    #[serde(default = "default_pad_to")]
    pub pad_to: u32,
    #[serde(default = "default_vram_per_mp")]
    pub vram_per_megapixel: f32,
    #[serde(default)]
    pub ms_per_megapixel_cpu: Option<f32>,
}

fn default_candidates() -> Vec<u32> {
    su_tiling::DEFAULT_CANDIDATES.to_vec()
}

fn default_overlap_divisor() -> u32 {
    16
}

fn default_pad_to() -> u32 {
    32
}

fn default_vram_per_mp() -> f32 {
    // Deliberadamente alto: mejor elegir un tile pequeno de mas en la primera
    // ejecucion que arriesgar un OOM por subestimar.
    600.0
}

impl Default for TilingHints {
    fn default() -> Self {
        Self {
            candidates: default_candidates(),
            overlap_divisor: default_overlap_divisor(),
            pad_to: default_pad_to(),
            vram_per_megapixel: default_vram_per_mp(),
            ms_per_megapixel_cpu: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub kind: ModelKind,
    /// Factor de escala nativo. 1 para modelos que no escalan (denoise, cara).
    pub scale: u32,

    #[serde(default)]
    pub arch: Option<String>,

    /// Hash del archivo ONNX. `None` = pendiente de verificar (se avisa).
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub urls: Vec<String>,
    /// Ruta relativa al directorio de modelos, para modelos ya presentes.
    #[serde(default)]
    pub local_path: Option<String>,

    pub license: License,
    #[serde(default)]
    pub precision: Precision,
    #[serde(default)]
    pub tiling: TilingHints,

    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,

    /// Menor = preferido en modo automatico dentro de su `kind`.
    #[serde(default)]
    pub auto_priority: Option<u32>,
}

impl ModelEntry {
    /// Nombre de archivo canonico: `<id>-<8 primeros del hash>.onnx`, o
    /// `<id>.onnx` si aun no hay hash.
    pub fn file_name(&self) -> String {
        match &self.sha256 {
            Some(hash) if hash.len() >= 8 => format!("{}-{}.onnx", self.id, &hash[..8]),
            _ => format!("{}.onnx", self.id),
        }
    }

    /// Ruta del modelo dentro del directorio de modelos.
    pub fn local_file(&self, models_dir: &Path) -> PathBuf {
        match &self.local_path {
            Some(relative) => models_dir.join(relative),
            None => models_dir.join(self.file_name()),
        }
    }

    /// Aviso legal que debe verse antes de descargar, si lo hay.
    pub fn license_warning(&self) -> Option<String> {
        if !self.license.commercial_use {
            Some(format!(
                "{} no permite uso comercial ({})",
                self.name, self.license.name
            ))
        } else {
            self.license.note.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccelerationPack {
    pub id: String,
    pub platform: Vec<String>,
    pub provider: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub requires: Option<PackRequirements>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRequirements {
    #[serde(default)]
    pub driver_min: Option<String>,
    #[serde(default)]
    pub gpu_vendor: Option<String>,
    #[serde(default)]
    pub compute_capability_min: Option<f32>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub manifest_version: u32,
    #[serde(default)]
    pub updated_at: Option<String>,
    pub models: Vec<ModelEntry>,
    #[serde(default)]
    pub acceleration_packs: Vec<AccelerationPack>,
}

/// Estado de un modelo respecto al cache local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelState {
    /// Presente y con el hash correcto.
    Installed,
    /// Presente pero el hash no coincide: hay que volver a descargarlo.
    HashMismatch,
    /// Presente, pero el manifiesto no declara hash con el que comprobarlo.
    Unverified,
    /// No esta en el cache.
    Missing,
}

/// Lo que necesita un descargador para traer un modelo: de donde, cuanto ocupa
/// y con que hash comprobarlo.
///
/// No incluye el destino: eso lo decide quien descarga, a partir del directorio
/// de modelos y de [`ModelEntry::file_name`]. Este crate describe, no descarga.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownload {
    /// Nombre canonico del archivo dentro del directorio de modelos.
    pub file_name: String,
    /// Espejos en orden de preferencia. Nunca vacio: sin URL no hay descarga.
    pub urls: Vec<String>,
    /// `sha256` en hexadecimal. Obligatorio: un archivo sin hash con el que
    /// comprobarlo no se descarga, porque no habria forma de saber si llego
    /// entero ni si es el modelo que dice ser.
    pub sha256: String,
    /// Tamano esperado en bytes. 0 = el manifiesto no lo declara, asi que el
    /// progreso solo puede ser indeterminado.
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub kind: ModelKind,
    pub scale: u32,
    pub state: ModelState,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub license_warning: Option<String>,
    /// Datos de descarga, o `None` si no hay nada que descargar.
    ///
    /// Es `None` en tres casos, y la interfaz debe distinguirlos por `state` y
    /// por `license_warning` en lugar de ofrecer un boton que no puede funcionar:
    /// el modelo ya esta instalado, el manifiesto apunta a un archivo local, o
    /// el manifiesto no declara ni URL ni hash.
    pub download: Option<ModelDownload>,
}

impl ModelStatus {
    /// Si se puede ofrecer la descarga: falta el archivo y hay de donde traerlo
    /// con un hash con el que comprobarlo.
    fn download_for(model: &ModelEntry, state: &ModelState) -> Option<ModelDownload> {
        if matches!(state, ModelState::Installed) {
            return None;
        }
        if model.local_path.is_some() || model.urls.is_empty() {
            return None;
        }
        let sha256 = model.sha256.clone()?;

        Some(ModelDownload {
            file_name: model.file_name(),
            urls: model.urls.clone(),
            sha256,
            size_bytes: model.size_bytes,
        })
    }
}

/// Catalogo de modelos en memoria.
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    manifest: Manifest,
}

impl ModelRegistry {
    pub fn from_json(raw: &str) -> SuResult<Self> {
        let manifest: Manifest = serde_json::from_str(raw).map_err(|error| {
            SuError::Internal(format!("manifiesto de modelos no valido: {error}"))
        })?;
        let registry = Self { manifest };
        registry.validate()?;
        Ok(registry)
    }

    pub fn from_file(path: &Path) -> SuResult<Self> {
        let raw = std::fs::read_to_string(path).map_err(|error| {
            SuError::Internal(format!("no se pudo leer {}: {error}", path.display()))
        })?;
        Self::from_json(&raw)
    }

    /// Comprobaciones que un manifiesto roto debe fallar antes de llegar al
    /// usuario: identificadores duplicados y escalas imposibles.
    fn validate(&self) -> SuResult<()> {
        let mut seen = HashSet::new();
        for model in &self.manifest.models {
            if !seen.insert(model.id.as_str()) {
                return Err(SuError::Internal(format!(
                    "el manifiesto tiene dos modelos con el id '{}'",
                    model.id
                )));
            }
            // Escalas admitidas: 1 para los modelos que no escalan (ruido, cara) y
            // 2, 4 u 8 para los base. Cualquier otra no es un valor raro, es un
            // error: con escala 3 el pipeline no puede componer la salida con
            // ninguna de las escalas que ofrece el producto, y el fallo apareceria
            // mucho mas tarde, al escribir una imagen del tamano equivocado.
            if !matches!(model.scale, 1 | 2 | 4 | 8) {
                return Err(SuError::Internal(format!(
                    "el modelo '{}' declara una escala invalida ({})",
                    model.id, model.scale
                )));
            }
            if model.tiling.candidates.is_empty() {
                return Err(SuError::Internal(format!(
                    "el modelo '{}' no declara ningun tamano de tile",
                    model.id
                )));
            }
        }
        Ok(())
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn models(&self) -> &[ModelEntry] {
        &self.manifest.models
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.manifest.models.iter().find(|model| model.id == id)
    }

    /// Modelos de un tipo concreto, ordenados por preferencia automatica.
    pub fn by_kind(&self, kind: ModelKind) -> Vec<&ModelEntry> {
        let mut found: Vec<&ModelEntry> = self
            .manifest
            .models
            .iter()
            .filter(|model| model.kind == kind)
            .collect();
        found.sort_by_key(|model| model.auto_priority.unwrap_or(u32::MAX));
        found
    }

    /// Modelo base preferido para un modo y una escala.
    pub fn preferred_base(&self, kind: ModelKind, scale: u32) -> Option<&ModelEntry> {
        self.by_kind(kind)
            .into_iter()
            .find(|model| model.scale == scale)
            .or_else(|| self.by_kind(kind).into_iter().next())
    }

    /// Estado de todos los modelos respecto al cache local.
    pub fn statuses(&self, models_dir: &Path) -> Vec<ModelStatus> {
        self.manifest
            .models
            .iter()
            .map(|model| {
                let path = model.local_file(models_dir);
                let state = self.state_of(model, &path);
                let download = ModelStatus::download_for(model, &state);
                let size_bytes = std::fs::metadata(&path)
                    .map(|meta| meta.len())
                    .unwrap_or(model.size_bytes);

                ModelStatus {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    kind: model.kind,
                    scale: model.scale,
                    state,
                    path,
                    size_bytes,
                    license_warning: model.license_warning(),
                    download,
                }
            })
            .collect()
    }

    pub fn state_of(&self, model: &ModelEntry, path: &Path) -> ModelState {
        if !path.exists() {
            return ModelState::Missing;
        }

        match &model.sha256 {
            None => ModelState::Unverified,
            Some(expected) => match sha256_file(path) {
                Ok(actual) if actual.eq_ignore_ascii_case(expected) => ModelState::Installed,
                Ok(_) => ModelState::HashMismatch,
                Err(_) => ModelState::Missing,
            },
        }
    }

    /// Resuelve el modelo que se debe usar, con su cadena de alternativas.
    ///
    /// Devuelve el primero instalado de la lista. Si ninguno lo esta, devuelve el
    /// error con el identificador del que faltaba, para que la UI pueda ofrecer
    /// descargarlo en lugar de dar un mensaje generico.
    pub fn resolve_installed<'a>(
        &'a self,
        ids: &[&str],
        models_dir: &Path,
    ) -> SuResult<(&'a ModelEntry, PathBuf)> {
        let mut first_missing: Option<String> = None;

        for id in ids {
            let Some(model) = self.get(id) else {
                continue;
            };
            let path = model.local_file(models_dir);
            match self.state_of(model, &path) {
                ModelState::Installed => return Ok((model, path)),
                ModelState::Unverified => return Ok((model, path)),
                ModelState::HashMismatch => {
                    return Err(SuError::ModelHashMismatch {
                        model: model.id.clone(),
                        expected: model.sha256.clone().unwrap_or_default(),
                        actual: sha256_file(&path).unwrap_or_else(|_| "ilegible".to_string()),
                    });
                }
                ModelState::Missing => {
                    if first_missing.is_none() {
                        first_missing = Some(model.id.clone());
                    }
                }
            }
        }

        Err(SuError::ModelMissing(
            first_missing.unwrap_or_else(|| ids.first().copied().unwrap_or("desconocido").to_string()),
        ))
    }
}

/// SHA-256 de un archivo, en hexadecimal minusculas.
///
/// Se lee por bloques: un modelo puede pesar cientos de megabytes y no tiene
/// sentido cargarlo entero en memoria solo para comprobarlo.
pub fn sha256_file(path: &Path) -> SuResult<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| SuError::ModelMissing(format!("{}: {error}", path.display())))?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| SuError::Internal(format!("leyendo {}: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(to_hex(&hasher.finalize()))
}

/// SHA-256 de unos bytes, para tests y para verificar manifiestos pequenos.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    to_hex(&hasher.finalize())
}

/// Codifica en hexadecimal minusculas.
///
/// Se hace a mano en lugar de con `format!("{:x}", ...)` porque el `LowerHex` de
/// la salida de `sha2` depende de una implementacion de `generic-array` que no
/// conviene dar por supuesta. Doce lineas quitan una incertidumbre.
fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("su-models-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("directorio temporal");
        dir
    }

    fn manifest_json(sha256: &str) -> String {
        format!(
            r#"{{
              "manifestVersion": 2,
              "models": [
                {{
                  "id": "4x-ultrasharp",
                  "name": "4x UltraSharp",
                  "kind": "photo",
                  "scale": 4,
                  "sha256": "{sha256}",
                  "sizeBytes": 67000000,
                  "license": {{ "name": "CC-BY-4.0", "commercialUse": true }},
                  "autoPriority": 1
                }},
                {{
                  "id": "realesrgan-x4plus",
                  "name": "Real-ESRGAN x4plus",
                  "kind": "photo",
                  "scale": 4,
                  "sha256": null,
                  "license": {{ "name": "BSD-3-Clause", "commercialUse": true }},
                  "autoPriority": 2
                }},
                {{
                  "id": "anime-6b",
                  "name": "RealESRGAN x4plus Anime 6B",
                  "kind": "illustration",
                  "scale": 4,
                  "sha256": null,
                  "license": {{ "name": "CC-BY-NC-SA-4.0", "commercialUse": false }}
                }}
              ]
            }}"#
        )
    }

    /// Un `sha256` con la longitud real (64 hexadecimales).
    ///
    /// Los tests que necesitan un nombre de archivo con sufijo usan este valor, no
    /// una cadena corta: el codigo exige al menos ocho caracteres para aceptar algo
    /// como hash, asi que un literal de seis no comprobaria lo que se cree.
    const HASH_COMPLETO: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn registry() -> ModelRegistry {
        ModelRegistry::from_json(&manifest_json(HASH_COMPLETO)).expect("manifiesto valido")
    }

    #[test]
    fn a_valid_manifest_parses_and_applies_defaults() {
        let registry = registry();
        assert_eq!(registry.models().len(), 3);

        let model = registry.get("4x-ultrasharp").expect("modelo");
        // Los valores por defecto de tiling se aplican solos.
        assert_eq!(model.tiling.pad_to, 32);
        assert_eq!(model.tiling.overlap_divisor, 16);
        assert_eq!(model.tiling.candidates, su_tiling::DEFAULT_CANDIDATES);
        assert!(model.precision.fp16_safe);
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let broken = r#"{
          "manifestVersion": 2,
          "models": [
            { "id": "a", "name": "A", "kind": "photo", "scale": 4, "license": {"name": "MIT"} },
            { "id": "a", "name": "A otra vez", "kind": "photo", "scale": 4, "license": {"name": "MIT"} }
          ]
        }"#;
        assert!(ModelRegistry::from_json(broken).is_err());
    }

    #[test]
    fn an_invalid_scale_is_rejected() {
        let broken = r#"{
          "manifestVersion": 2,
          "models": [
            { "id": "a", "name": "A", "kind": "photo", "scale": 3, "license": {"name": "MIT"} }
          ]
        }"#;
        assert!(ModelRegistry::from_json(broken).is_err());
    }

    #[test]
    fn a_model_without_tile_candidates_is_rejected() {
        let broken = r#"{
          "manifestVersion": 2,
          "models": [
            { "id": "a", "name": "A", "kind": "photo", "scale": 4,
              "license": {"name": "MIT"}, "tiling": { "candidates": [] } }
          ]
        }"#;
        assert!(ModelRegistry::from_json(broken).is_err());
    }

    #[test]
    fn an_unknown_kind_is_rejected() {
        let broken = r#"{
          "manifestVersion": 2,
          "models": [
            { "id": "a", "name": "A", "kind": "magia", "scale": 4, "license": {"name": "MIT"} }
          ]
        }"#;
        assert!(ModelRegistry::from_json(broken).is_err());
    }

    #[test]
    fn file_names_include_a_hash_prefix_when_available() {
        let registry = registry();
        assert_eq!(
            registry.get("4x-ultrasharp").unwrap().file_name(),
            "4x-ultrasharp-aaaaaaaa.onnx"
        );
        // Sin hash, el nombre no lleva sufijo.
        assert_eq!(
            registry.get("realesrgan-x4plus").unwrap().file_name(),
            "realesrgan-x4plus.onnx"
        );
    }

    #[test]
    fn a_missing_model_reports_missing() {
        let dir = scratch("missing");
        let statuses = registry().statuses(&dir);
        assert!(statuses.iter().all(|status| status.state == ModelState::Missing));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_model_without_a_hash_is_reported_as_unverified_not_as_installed() {
        let dir = scratch("unverified");
        let registry = registry();
        let model = registry.get("realesrgan-x4plus").unwrap();
        let path = model.local_file(&dir);
        std::fs::write(&path, b"pesos falsos").expect("escritura");

        // Este es el punto: sin hash no se puede afirmar que el archivo es el
        // correcto, asi que no se miente diciendo "instalado".
        assert_eq!(registry.state_of(model, &path), ModelState::Unverified);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_correct_hash_reports_installed() {
        let dir = scratch("installed");
        let content = b"pesos de verdad";
        let hash = sha256_bytes(content);
        let registry = ModelRegistry::from_json(&manifest_json(&hash)).expect("manifiesto");

        let model = registry.get("4x-ultrasharp").unwrap();
        let path = model.local_file(&dir);
        std::fs::write(&path, content).expect("escritura");

        assert_eq!(registry.state_of(model, &path), ModelState::Installed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_hash_reports_mismatch() {
        let dir = scratch("mismatch");
        let registry = registry(); // espera "abc123"
        let model = registry.get("4x-ultrasharp").unwrap();
        let path = model.local_file(&dir);
        std::fs::write(&path, b"otro contenido").expect("escritura");

        assert_eq!(registry.state_of(model, &path), ModelState::HashMismatch);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashing_is_stable_and_matches_the_known_vector() {
        // Vector conocido: SHA-256 de "abc".
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hashing_a_file_matches_hashing_its_bytes() {
        let dir = scratch("hashfile");
        let path = dir.join("dato.bin");
        let content = b"contenido de prueba para el hash";
        std::fs::write(&path, content).expect("escritura");

        assert_eq!(sha256_file(&path).unwrap(), sha256_bytes(content));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashing_a_missing_file_fails_clearly() {
        assert!(sha256_file(Path::new("/no/existe.bin")).is_err());
    }

    #[test]
    fn base_models_are_ordered_by_auto_priority() {
        let registry = registry();
        let photos = registry.by_kind(ModelKind::Photo);
        assert_eq!(photos.len(), 2);
        assert_eq!(photos[0].id, "4x-ultrasharp");
        assert_eq!(photos[1].id, "realesrgan-x4plus");
    }

    #[test]
    fn the_preferred_base_model_is_the_one_for_the_scale() {
        let registry = registry();
        let preferred = registry
            .preferred_base(ModelKind::Photo, 4)
            .expect("modelo base");
        assert_eq!(preferred.id, "4x-ultrasharp");

        // Si no hay modelo para esa escala, se devuelve el preferido del tipo.
        let fallback = registry
            .preferred_base(ModelKind::Photo, 8)
            .expect("modelo base");
        assert_eq!(fallback.id, "4x-ultrasharp");
    }

    #[test]
    fn a_non_commercial_license_produces_a_warning() {
        let registry = registry();
        let warning = registry
            .get("anime-6b")
            .unwrap()
            .license_warning()
            .expect("deberia avisar");
        assert!(warning.contains("no permite uso comercial"), "{warning}");

        // Una licencia permisiva sin nota no genera aviso.
        assert!(registry.get("4x-ultrasharp").unwrap().license_warning().is_none());
    }

    #[test]
    fn resolving_falls_back_to_the_next_installed_model() {
        let dir = scratch("resolve");
        let registry = registry();

        // Solo esta el segundo de la lista.
        let second = registry.get("realesrgan-x4plus").unwrap();
        std::fs::write(second.local_file(&dir), b"pesos").expect("escritura");

        let (resolved, _path) = registry
            .resolve_installed(&["4x-ultrasharp", "realesrgan-x4plus"], &dir)
            .expect("deberia caer al segundo");
        assert_eq!(resolved.id, "realesrgan-x4plus");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolving_reports_the_missing_model_by_name() {
        let dir = scratch("resolve-missing");
        let error = registry()
            .resolve_installed(&["4x-ultrasharp"], &dir)
            .unwrap_err();

        assert_eq!(error.code().as_str(), "SU-E110");
        assert!(error.to_string().contains("4x-ultrasharp"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolving_a_corrupted_model_reports_the_hash_mismatch() {
        let dir = scratch("resolve-corrupt");
        let registry = registry();
        let model = registry.get("4x-ultrasharp").unwrap();
        std::fs::write(model.local_file(&dir), b"corrupto").expect("escritura");

        // `registry` ya esta ligado arriba; volver a llamar a la funcion del
        // mismo nombre exigiria renombrar el binding, y no hace falta: el
        // registro es inmutable.
        let error = registry
            .resolve_installed(&["4x-ultrasharp"], &dir)
            .unwrap_err();

        assert_eq!(error.code().as_str(), "SU-E111");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_path_overrides_the_generated_name() {
        let json = r#"{
          "manifestVersion": 2,
          "models": [
            { "id": "custom", "name": "Custom", "kind": "photo", "scale": 4,
              "localPath": "sub/carpeta/modelo.onnx",
              "license": {"name": "MIT"} }
          ]
        }"#;
        let registry = ModelRegistry::from_json(json).unwrap();
        let model = registry.get("custom").unwrap();
        let path = model.local_file(Path::new("/modelos"));
        assert!(path.to_string_lossy().ends_with("sub/carpeta/modelo.onnx"));
    }

    /// Manifiesto con las cuatro combinaciones que deciden si algo se puede
    /// descargar: hash + URL, URL sin hash, hash con `localPath`, y hash sin URL.
    fn download_manifest(hash: &str) -> String {
        format!(
            r#"{{
              "manifestVersion": 2,
              "models": [
                {{ "id": "completo", "name": "Completo", "kind": "photo", "scale": 4,
                  "sha256": "{hash}", "sizeBytes": 33605809,
                  "urls": ["https://ejemplo.invalido/a.onnx", "https://espejo.invalido/a.onnx"],
                  "license": {{ "name": "CC-BY-NC-SA-4.0", "commercialUse": false }} }},
                {{ "id": "sin-hash", "name": "Sin hash", "kind": "photo", "scale": 4,
                  "sizeBytes": 100,
                  "urls": ["https://ejemplo.invalido/b.onnx"],
                  "license": {{ "name": "MIT", "commercialUse": true }} }},
                {{ "id": "local", "name": "Local", "kind": "photo", "scale": 4,
                  "sha256": "{hash}", "localPath": "sub/local.onnx",
                  "urls": ["https://ejemplo.invalido/c.onnx"],
                  "license": {{ "name": "MIT", "commercialUse": true }} }},
                {{ "id": "sin-url", "name": "Sin URL", "kind": "photo", "scale": 4,
                  "sha256": "{hash}",
                  "license": {{ "name": "MIT", "commercialUse": true }} }}
              ]
            }}"#
        )
    }

    fn status_of<'a>(statuses: &'a [ModelStatus], id: &str) -> &'a ModelStatus {
        statuses
            .iter()
            .find(|status| status.id == id)
            .expect("el manifiesto declara ese modelo")
    }

    #[test]
    fn a_missing_model_with_a_url_and_a_hash_is_offered_for_download() {
        let dir = scratch("dl-ofrecido");
        let registry = ModelRegistry::from_json(&download_manifest(HASH_COMPLETO)).unwrap();

        let statuses = registry.statuses(&dir);
        let status = status_of(&statuses, "completo");
        assert_eq!(status.state, ModelState::Missing);

        let download = status.download.as_ref().expect("deberia poder descargarse");
        // El nombre lleva el hash por delante: un archivo suelto en el directorio
        // dice a que modelo pertenece y con que version se comprobo.
        assert_eq!(download.file_name, "completo-aaaaaaaa.onnx");
        assert_eq!(download.sha256, HASH_COMPLETO);
        assert_eq!(download.size_bytes, 33605809);
        // Se conservan todos los espejos, en orden: si el primero cae, queda el otro.
        assert_eq!(download.urls.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_hash_there_is_no_download_even_with_a_url() {
        // Descargar algo que no se puede comprobar es justo lo que este proyecto
        // no hace: mejor decir que no se puede que traer un archivo del que no
        // se sabe si es el modelo que dice ser.
        let dir = scratch("dl-sin-hash");
        let registry = ModelRegistry::from_json(&download_manifest(HASH_COMPLETO)).unwrap();

        let statuses = registry.statuses(&dir);
        let status = status_of(&statuses, "sin-hash");
        assert_eq!(status.state, ModelState::Missing);
        assert!(status.download.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_local_model_is_never_downloaded() {
        // `localPath` significa "este archivo lo pone el usuario": descargar
        // encima pisaria su copia.
        let dir = scratch("dl-local");
        let registry = ModelRegistry::from_json(&download_manifest(HASH_COMPLETO)).unwrap();

        let statuses = registry.statuses(&dir);
        assert!(status_of(&statuses, "local").download.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_model_without_a_url_offers_no_download() {
        let dir = scratch("dl-sin-url");
        let registry = ModelRegistry::from_json(&download_manifest(HASH_COMPLETO)).unwrap();

        let statuses = registry.statuses(&dir);
        assert!(status_of(&statuses, "sin-url").download.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_installed_model_is_not_offered_for_download() {
        let dir = scratch("dl-instalado");
        let content = b"pesos de verdad";
        let hash = sha256_bytes(content);
        let registry = ModelRegistry::from_json(&download_manifest(&hash)).unwrap();

        let model = registry.get("completo").unwrap();
        std::fs::write(model.local_file(&dir), content).expect("escritura");

        let statuses = registry.statuses(&dir);
        let status = status_of(&statuses, "completo");
        assert_eq!(status.state, ModelState::Installed);
        assert!(
            status.download.is_none(),
            "ya esta instalado: no hay nada que ofrecer"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hash_mismatch_offers_the_download_again() {
        // Este es el caso que hace util el boton: el archivo esta, pero no es el
        // bueno. Sin poder volver a descargar, el modelo queda inservible.
        let dir = scratch("dl-mismatch");
        let registry = ModelRegistry::from_json(&download_manifest(HASH_COMPLETO)).unwrap();

        let model = registry.get("completo").unwrap();
        std::fs::write(model.local_file(&dir), b"contenido equivocado").expect("escritura");

        let statuses = registry.statuses(&dir);
        let status = status_of(&statuses, "completo");
        assert_eq!(status.state, ModelState::HashMismatch);
        assert!(status.download.is_some(), "hay que poder recuperarse");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
