//! Backend de inferencia sobre ONNX Runtime.
//!
//! Vive detras de la feature `onnx` (ver ADR-018) para que el resto del sidecar
//! se pueda compilar y testear sin descargar ni enlazar ORT.
//!
//! ## Lo que hace y lo que no
//!
//! Este modulo solo sabe de **un tile**: recibe pixeles entrelazados, los pasa al
//! modelo y devuelve el resultado. No conoce el solape, ni la VRAM, ni la imagen
//! completa: de eso se encargan `su-tiling` y `su-inference::runner`. Mantener esa
//! frontera es lo que permite que la composicion se verifique sin GPU.
//!
//! ## Detalle que importa: el orden de los canales
//!
//! Los modelos ONNX esperan `NCHW` (planar) y el resto del pipeline trabaja en
//! `HWC` (entrelazado). La conversion es la parte mas facil de equivocar de todo
//! el modulo —un error aqui produce una imagen con los canales intercambiados,
//! que es un fallo que se ve pero se diagnostica mal— asi que esta aislada en dos
//! funciones puras con tests propios.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ort::ep::{ExecutionProviderDispatch, CPU, CUDA, CoreML, DirectML, TensorRT};
use ort::session::Session;
use ort::value::Tensor;
use su_core::{SuError, SuResult};
use su_models::ModelRegistry;
use su_tiling::ProviderKind;

use crate::backend::{Backend, TileInput, TileOutput};
use crate::runner::{BackendProvider, TilingHints};

/// Ajustes de carga de un modelo.
#[derive(Debug, Clone)]
pub struct OrtConfig {
    pub model_path: PathBuf,
    pub provider: ProviderKind,
    /// Indice de dispositivo. 0 es la primera GPU.
    pub device_id: u32,
    /// Identificador del modelo, para logs y para el resumen del lote.
    pub model_id: String,
    /// Factor de escala nativo del modelo.
    pub scale: u32,
    /// Hilos del EP de CPU. 0 deja que decida ONNX Runtime.
    ///
    /// Se aplica **solo** al EP de CPU: en GPU el cuello de botella es el
    /// dispositivo, no los hilos de la CPU, y fijarlos ahi no aporta nada.
    pub intra_threads: usize,
}

/// Sesion de ONNX Runtime compartida.
///
/// Va en un `Arc<Mutex<_>>` porque `Session::run` exige `&mut self`: ONNX Runtime
/// no permite dos ejecuciones simultaneas sobre la misma sesion. Con una sola
/// sesion por modelo, el mutex no es contencion real, es la forma de expresar esa
/// restriccion en el sistema de tipos.
type SharedSession = Arc<Mutex<Session>>;

pub struct OrtBackend {
    session: SharedSession,
    id: String,
    scale: u32,
}

impl OrtBackend {
    pub fn load(config: OrtConfig) -> SuResult<Self> {
        if !config.model_path.exists() {
            return Err(SuError::ModelMissing(config.model_path.display().to_string()));
        }

        let session = load_session(&config)?;

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            id: config.model_id,
            scale: config.scale,
        })
    }
}

fn load_session(config: &OrtConfig) -> SuResult<Session> {
    let providers = dispatch_for(config.provider, config.device_id);

    let mut builder = Session::builder()
        .map_err(ort_error)?
        .with_execution_providers(providers)
        .map_err(ort_error)?;

    // ADR-014: en CPU se usan los nucleos **fisicos** y un solo hilo entre
    // operadores. El hyperthreading no ayuda a las convoluciones, que ya saturan
    // las unidades de coma flotante, y varias sesiones en paralelo compiten por
    // el mismo ancho de banda de memoria.
    if config.provider == ProviderKind::Cpu && config.intra_threads > 0 {
        builder = builder
            .with_intra_threads(config.intra_threads)
            .map_err(ort_error)?
            .with_inter_threads(1)
            .map_err(ort_error)?;
    }

    builder
        .commit_from_file(&config.model_path)
        .map_err(|error| {
            // Un fallo al cargar casi siempre significa que el EP elegido no
            // arranca (falta la biblioteca del proveedor) o que el modelo es
            // incompatible. Conviene distinguirlo de "el modelo no esta".
            SuError::ExecutionProviderUnavailable {
                provider: config.provider.as_str().to_string(),
                reason: error.to_string(),
            }
        })
}

/// Traduce un EP del dominio al dispatch de ONNX Runtime.
///
/// Las variantes existen siempre porque la dependencia activa las features de
/// todos los EPs: lo que decide si funcionan es que la biblioteca del proveedor
/// este presente en tiempo de ejecucion, no que el codigo las conozca.
fn dispatch_for(provider: ProviderKind, device_id: u32) -> Vec<ExecutionProviderDispatch> {
    let device = device_id as i32;

    match provider {
        ProviderKind::TensorRt => vec![TensorRT::default().with_device_id(device).build()],
        ProviderKind::Cuda => vec![CUDA::default().with_device_id(device).build()],
        ProviderKind::DirectMl => vec![DirectML::default().with_device_id(device).build()],
        ProviderKind::CoreMl => vec![CoreML::default().build()],
        ProviderKind::Cpu => vec![CPU::default().build()],
    }
}

impl std::fmt::Debug for OrtBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // La sesion de ORT no se imprime: no aporta nada en un log, no es barata
        // de recorrer y no expone nada util. Se muestran los datos que si
        // identifican al backend, que es lo que se busca al depurar.
        f.debug_struct("OrtBackend")
            .field("id", &self.id)
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl Backend for OrtBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn scale(&self) -> u32 {
        self.scale
    }

    fn run_tile(&mut self, input: &TileInput<'_>) -> SuResult<TileOutput> {
        let channels = input.channels;
        let height = input.height as usize;
        let width = input.width as usize;

        let planar = to_nchw(input.data, channels, height, width);
        let shape = vec![1i64, channels as i64, height as i64, width as i64];

        let tensor = Tensor::from_array((shape, planar)).map_err(ort_error)?;

        let mut session = self.session.lock().map_err(|_| {
            SuError::Internal("la sesion de inferencia esta bloqueada".to_string())
        })?;

        let outputs = session
            .run(ort::inputs![tensor])
            .map_err(classify_run_error)?;

        let (_name, value) = outputs.iter().next().ok_or_else(|| {
            SuError::Internal("el modelo no devolvio ninguna salida".to_string())
        })?;

        let (out_shape, data) = value.try_extract_tensor::<f32>().map_err(ort_error)?;

        let dims: Vec<i64> = out_shape.iter().copied().collect();
        if dims.len() != 4 {
            return Err(SuError::Internal(format!(
                "la salida del modelo tiene {} dimensiones, se esperaban 4 (NCHW)",
                dims.len()
            )));
        }

        let out_channels = dims[1] as usize;
        let out_height = dims[2] as usize;
        let out_width = dims[3] as usize;

        if out_channels != channels {
            return Err(SuError::Internal(format!(
                "el modelo devolvio {out_channels} canales y se le dieron {channels}"
            )));
        }

        let interleaved = from_nchw(data, out_channels, out_height, out_width);

        TileOutput::new(
            out_width as u32,
            out_height as u32,
            out_channels,
            interleaved,
        )
    }
}

/// `HWC` entrelazado -> `NCHW` planar.
pub fn to_nchw(data: &[f32], channels: usize, height: usize, width: usize) -> Vec<f32> {
    let mut planar = vec![0.0f32; channels * height * width];
    let plane = height * width;

    for y in 0..height {
        for x in 0..width {
            let source = (y * width + x) * channels;
            for channel in 0..channels {
                planar[channel * plane + y * width + x] = data[source + channel];
            }
        }
    }

    planar
}

/// `NCHW` planar -> `HWC` entrelazado.
pub fn from_nchw(data: &[f32], channels: usize, height: usize, width: usize) -> Vec<f32> {
    let mut interleaved = vec![0.0f32; channels * height * width];
    let plane = height * width;

    for y in 0..height {
        for x in 0..width {
            let target = (y * width + x) * channels;
            for channel in 0..channels {
                interleaved[target + channel] = data[channel * plane + y * width + x];
            }
        }
    }

    interleaved
}

/// Traduce un fallo de ONNX Runtime a un error de dominio.
///
/// La crate `ort` parametriza sus errores con el tipo que se estaba construyendo
/// (`Error<SessionBuilder>` al crear la sesion, `Error<()>` al ejecutar), asi que
/// el parametro se deja abierto a proposito: a este ayudante solo le interesa el
/// texto del fallo, no de donde vino.
fn ort_error<R>(error: ort::Error<R>) -> SuError {
    SuError::Internal(format!("ONNX Runtime: {error}"))
}

/// Traduce un fallo de ejecucion al error de dominio correspondiente.
///
/// Solo los fallos de memoria se marcan como recuperables: son los unicos que la
/// escalera de degradacion puede resolver reduciendo el tile. Marcar como
/// recuperable un error de operador no soportado solo haria perder el tiempo
/// reintentando con tiles cada vez mas pequenos.
fn classify_run_error(error: ort::Error) -> SuError {
    let text = error.to_string();
    let lower = text.to_ascii_lowercase();

    let is_memory = lower.contains("out of memory")
        || lower.contains("outofmemory")
        || lower.contains("failed to allocate")
        || lower.contains("memory allocation")
        || lower.contains("insufficient memory");

    if is_memory {
        // El tile y la VRAM no se conocen en este punto; el runner los anota en
        // su propio mensaje de degradacion.
        SuError::OutOfVram {
            tile: 0,
            free_mb: 0,
            needed_mb: 0,
        }
    } else {
        SuError::Internal(format!("inferencia: {text}"))
    }
}

// ---------------------------------------------------------------------------
// Localizacion y comprobacion del runtime
// ---------------------------------------------------------------------------

/// Raices de nombre de la biblioteca del **nucleo** de ONNX Runtime.
///
/// El nucleo, no los execution providers: cargar `onnxruntime_providers_cuda`
/// directamente no sirve de nada, y confundir los dos convertiria "tengo la
/// biblioteca de CUDA" en "tengo ONNX Runtime".
const RUNTIME_LIBRARY_STEMS: &[&str] = &["libonnxruntime", "onnxruntime"];

/// Elige, entre los nombres de un directorio, la biblioteca del nucleo de ORT.
///
/// Es una funcion pura sobre la lista de nombres para poder comprobarla sin
/// copiar 24 MB de biblioteca en cada prueba. Se prefiere el nombre sin version
/// (`libonnxruntime.so`) y, si no esta, el versionado mas corto: `dlopen` acepta
/// los dos, pero el canonico es el que sobrevive a una actualizacion de la
/// biblioteca y el que un enlace estable puede usar.
pub fn pick_runtime_library(names: &[String]) -> Option<String> {
    let mut candidates: Vec<(u8, usize, &str)> = Vec::new();

    for name in names {
        let lower = name.to_ascii_lowercase();

        // Los execution providers no son el runtime.
        if lower.contains("providers") {
            continue;
        }

        let Some(stem) = RUNTIME_LIBRARY_STEMS
            .iter()
            .find(|stem| lower.starts_with(**stem))
        else {
            continue;
        };

        let is_library = lower.ends_with(".so")
            || lower.ends_with(".dylib")
            || lower.ends_with(".dll")
            // `libonnxruntime.so.1.28.2`: versionado a la manera de Linux.
            || lower.contains(".so.");

        if !is_library {
            continue;
        }

        let canonical = lower == format!("{stem}.so")
            || lower == format!("{stem}.dylib")
            || lower == format!("{stem}.dll");

        candidates.push((u8::from(!canonical), name.len(), name.as_str()));
    }

    candidates.sort_by_key(|(exact, length, _)| (*exact, *length));
    candidates.first().map(|(_, _, name)| (*name).to_string())
}

/// Busca la biblioteca del nucleo dentro de un directorio.
///
/// Devuelve `None` si el directorio no existe o no hay ninguna: no poder mirar no
/// es un error en si mismo, es "no lo se" (el llamador decide que hacer).
pub fn find_runtime_library(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;

    let names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();

    pick_runtime_library(&names).map(|name| dir.join(name))
}

/// Comprueba que ONNX Runtime se puede cargar **de verdad** y devuelve su ruta.
///
/// No basta con que el archivo exista: la version de la biblioteca tiene que
/// coincidir con la que espera el enlace compilado, y eso solo se sabe intentando
/// cargarla. Es exactamente la clase de fallo que el proyecto quiere detectar
/// **antes** de aceptar un trabajo: si el runtime no arranca, el motor tiene que
/// decirlo y usar el clasico, no aceptar la imagen y fallar a mitad.
pub fn probe_runtime(exe_dir: &Path) -> Result<PathBuf, String> {
    probe_runtime_with(exe_dir, std::env::var("ORT_DYLIB_PATH").ok().as_deref())
}

/// Igual que [`probe_runtime`], con el valor de `ORT_DYLIB_PATH` recibido como
/// parametro.
///
/// Se separa de la lectura del entorno por el mismo motivo que el token del
/// servidor: en un test las variables de entorno son compartidas por todos los
/// hilos, asi que una prueba que dependa de ellas es una prueba intermitente.
/// Aqui, ademas, decidir entre "cargar" y "buscar al lado del ejecutable" es
/// exactamente lo que hay que poder comprobar.
pub fn probe_runtime_with(exe_dir: &Path, from_env: Option<&str>) -> Result<PathBuf, String> {
    // 1. La variable de entorno manda. Permite apuntar a un runtime concreto
    //    (una version con CUDA, otro directorio) sin tocar el binario.
    if let Some(value) = from_env.map(str::trim).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        return ort::init_from(&path).map(|_| path.clone()).map_err(|error| {
            format!(
                "ORT_DYLIB_PATH apunta a '{}', que no se pudo cargar: {error}",
                path.display()
            )
        });
    }

    // 2. Al lado del ejecutable, que es la convencion documentada en docs/06.
    let Some(path) = find_runtime_library(exe_dir) else {
        return Err(format!(
            "no se encontro la biblioteca de ONNX Runtime en {} (se esperaba libonnxruntime.so, onnxruntime.dll o ORT_DYLIB_PATH)",
            exe_dir.display()
        ));
    };

    ort::init_from(&path)
        .map(|_| path.clone())
        .map_err(|error| format!("no se pudo cargar {}: {error}", path.display()))
}

/// Fabrica de backends sobre ONNX Runtime, con cache de sesiones.
///
/// La cache no es una optimizacion opcional: cargar un modelo de 67 MB en cada
/// imagen de un lote de cien convertiria un trabajo de minutos en uno de horas.
/// La clave es el identificador del modelo; el EP y el dispositivo son fijos
/// durante toda la vida del proveedor.
pub struct OrtProvider {
    models_dir: PathBuf,
    registry: Arc<ModelRegistry>,
    provider: ProviderKind,
    device_id: u32,
    /// Nucleos fisicos a usar por el EP de CPU. Ver ADR-014.
    intra_threads: usize,
    sessions: Mutex<HashMap<String, SharedSession>>,
}

impl OrtProvider {
    pub fn new(
        models_dir: PathBuf,
        registry: Arc<ModelRegistry>,
        provider: ProviderKind,
        device_id: u32,
        intra_threads: usize,
    ) -> Self {
        Self {
            models_dir,
            registry,
            provider,
            device_id,
            intra_threads,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Cuantas sesiones hay cargadas. Util para saber si conviene liberar memoria
    /// entre imagenes.
    pub fn loaded_sessions(&self) -> usize {
        self.sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    /// Descarta las sesiones cargadas. Se usa con `unloadBetweenImages` en GPUs
    /// con poca memoria.
    pub fn unload_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
    }
}

impl BackendProvider for OrtProvider {
    fn name(&self) -> &str {
        self.provider.as_str()
    }

    fn uses_vram(&self) -> bool {
        self.provider.uses_vram()
    }

    fn unload(&self) {
        self.unload_all()
    }

    fn native_scale(&self, model_id: &str) -> Option<u32> {
        // El manifiesto ya declara la escala de cada modelo, asi que no hace falta
        // abrir una sesion de ONNX Runtime para leerla.
        self.registry.get(model_id).map(|model| model.scale)
    }

    fn tiling_hints(&self, model_id: &str) -> Option<TilingHints> {
        // El manifiesto declara el tile, el solape, la alineacion y el consumo de
        // VRAM de cada modelo. Es lo que sabe su autor y no lo que se supone por
        // defecto para todos: un modelo que declare `candidates: [768, 512]` porque
        // el tile 1024 le hace producir artefactos tiene que recibir 768.
        self.registry.get(model_id).map(|model| TilingHints {
            candidates: model.tiling.candidates.clone(),
            overlap_divisor: model.tiling.overlap_divisor,
            pad_to: model.tiling.pad_to,
            vram_per_megapixel: Some(model.tiling.vram_per_megapixel),
        })
    }

    fn backend_for(&self, model_id: &str) -> SuResult<Box<dyn Backend>> {
        let model = self
            .registry
            .get(model_id)
            .ok_or_else(|| SuError::ModelMissing(model_id.to_string()))?;

        let path = model.local_file(&self.models_dir);

        if let Ok(sessions) = self.sessions.lock() {
            if let Some(session) = sessions.get(model_id) {
                return Ok(Box::new(OrtBackend {
                    session: Arc::clone(session),
                    id: model_id.to_string(),
                    scale: model.scale,
                }));
            }
        }

        let config = OrtConfig {
            model_path: path,
            provider: self.provider,
            device_id: self.device_id,
            model_id: model_id.to_string(),
            scale: model.scale,
            intra_threads: self.intra_threads,
        };

        let backend = OrtBackend::load(config)?;

        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(model_id.to_string(), Arc::clone(&backend.session));
        }

        Ok(Box::new(backend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nchw_conversion_reorders_channels() {
        // Un pixel con los canales bien distintos: si la conversion estuviera
        // mal, se veria el rojo donde va el azul.
        let hwc = vec![
            1.0, 2.0, 3.0, // pixel (0,0)
            4.0, 5.0, 6.0, // pixel (1,0)
        ];

        let planar = to_nchw(&hwc, 3, 1, 2);
        // Canal 0 (rojo) de los dos pixeles, luego canal 1, luego canal 2.
        assert_eq!(planar, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn nchw_round_trips() {
        let original: Vec<f32> = (0..(4 * 3 * 3)).map(|value| value as f32).collect();

        let planar = to_nchw(&original, 3, 4, 3);
        let restored = from_nchw(&planar, 3, 4, 3);

        assert_eq!(restored, original);
    }

    #[test]
    fn nchw_round_trips_with_a_single_channel() {
        let original: Vec<f32> = (0..12).map(|value| value as f32 / 12.0).collect();

        let planar = to_nchw(&original, 1, 3, 4);
        // Con un canal, planar y entrelazado son lo mismo.
        assert_eq!(planar, original);
        assert_eq!(from_nchw(&planar, 1, 3, 4), original);
    }

    #[test]
    fn planar_layout_is_channel_major() {
        // 2x1 con dos canales: [R0, G0, R1, G1] -> [R0, R1, G0, G1]
        let hwc = vec![10.0, 20.0, 30.0, 40.0];
        let planar = to_nchw(&hwc, 2, 1, 2);
        assert_eq!(planar, vec![10.0, 30.0, 20.0, 40.0]);
    }

    #[test]
    fn a_missing_model_is_reported_before_touching_onnx_runtime() {
        // La comprobacion de existencia va primero: no tiene sentido pagar el
        // coste de crear una sesion para descubrir que el archivo no esta.
        let error = OrtBackend::load(OrtConfig {
            model_path: PathBuf::from("/no/existe/modelo.onnx"),
            provider: ProviderKind::Cpu,
            device_id: 0,
            model_id: "prueba".to_string(),
            scale: 4,
            intra_threads: 4,
        })
        .unwrap_err();

        assert_eq!(error.code().as_str(), "SU-E110");
    }

    #[test]
    fn every_provider_produces_a_dispatch() {
        // Comprobacion barata de que el mapeo cubre todas las variantes y no
        // deja ninguna sin construir.
        for provider in [
            ProviderKind::TensorRt,
            ProviderKind::Cuda,
            ProviderKind::DirectMl,
            ProviderKind::CoreMl,
            ProviderKind::Cpu,
        ] {
            let dispatch = dispatch_for(provider, 0);
            assert_eq!(dispatch.len(), 1, "{provider:?} no produjo dispatch");
        }
    }

    #[test]
    fn a_memory_error_is_classified_as_recoverable() {
        // No se puede construir un `ort::Error` a mano, asi que se comprueba la
        // logica de clasificacion por separado mediante su predicado.
        for text in [
            "CUDA out of memory",
            "Failed to allocate memory for tensor",
            "OrtOutOfMemory",
            "insufficient memory",
        ] {
            assert!(looks_like_memory_error(text), "{text}");
        }

        for text in [
            "Unsupported operator",
            "Invalid model",
            "shape mismatch",
        ] {
            assert!(!looks_like_memory_error(text), "{text}");
        }
    }

    /// Misma logica que `classify_run_error`, expuesta para poder testearla sin
    /// construir un error de ONNX Runtime.
    fn looks_like_memory_error(text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        lower.contains("out of memory")
            || lower.contains("outofmemory")
            || lower.contains("failed to allocate")
            || lower.contains("memory allocation")
            || lower.contains("insufficient memory")
    }

    #[test]
    fn the_provider_starts_with_no_cached_sessions() {
        let registry = Arc::new(
            ModelRegistry::from_json(r#"{"manifestVersion":2,"models":[]}"#).expect("manifiesto"),
        );
        let provider = OrtProvider::new(
            PathBuf::from("/modelos"),
            registry,
            ProviderKind::Cpu,
            0,
            4,
        );
        assert_eq!(provider.loaded_sessions(), 0);
    }

    #[test]
    fn the_provider_rejects_a_model_that_is_not_in_the_catalogue() {
        let registry = Arc::new(
            ModelRegistry::from_json(r#"{"manifestVersion":2,"models":[]}"#).expect("manifiesto"),
        );
        let provider = OrtProvider::new(
            PathBuf::from("/modelos"),
            registry,
            ProviderKind::Cpu,
            0,
            4,
        );

        let error = provider.backend_for("no-existe").unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E110");
    }

    #[test]
    fn the_runtime_library_is_picked_before_the_execution_providers() {
        // Un directorio real de ONNX Runtime: la biblioteca del nucleo, la de los
        // proveedores compartidos y las de CUDA/TensorRT. Elegir cualquiera de las
        // ultimas seria cargar un EP como si fuera el motor.
        let names: Vec<String> = [
            "libonnxruntime.so.1.28.2",
            "libonnxruntime.so.1",
            "libonnxruntime.so",
            "libonnxruntime_providers_shared.so",
            "libonnxruntime_providers_cuda.so",
            "libonnxruntime_providers_tensorrt.so",
            "su-cli",
        ]
        .iter()
        .map(|name| (*name).to_string())
        .collect();

        assert_eq!(pick_runtime_library(&names).as_deref(), Some("libonnxruntime.so"));
    }

    #[test]
    fn a_directory_with_only_providers_has_no_runtime() {
        let names: Vec<String> = [
            "libonnxruntime_providers_shared.so",
            "libonnxruntime_providers_cuda.so",
        ]
        .iter()
        .map(|name| (*name).to_string())
        .collect();

        assert_eq!(pick_runtime_library(&names), None);
    }

    #[test]
    fn a_versioned_library_is_used_when_there_is_no_canonical_one() {
        let names: Vec<String> = ["libonnxruntime.so.1.28.2", "libonnxruntime_providers_shared.so"]
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        assert_eq!(
            pick_runtime_library(&names).as_deref(),
            Some("libonnxruntime.so.1.28.2")
        );
    }

    #[test]
    fn the_windows_and_macos_names_are_recognised_too() {
        let windows: Vec<String> = vec!["onnxruntime.dll".to_string()];
        assert_eq!(pick_runtime_library(&windows).as_deref(), Some("onnxruntime.dll"));

        let macos: Vec<String> = vec!["libonnxruntime.1.28.2.dylib".to_string()];
        assert_eq!(
            pick_runtime_library(&macos).as_deref(),
            Some("libonnxruntime.1.28.2.dylib")
        );
    }

    #[test]
    fn a_file_that_is_not_a_library_is_ignored() {
        let names: Vec<String> = ["onnxruntime-gpu.txt", "notas-onnxruntime.md"]
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        assert_eq!(pick_runtime_library(&names), None);
    }

    #[test]
    fn probing_a_directory_without_the_runtime_reports_where_it_looked() {
        // El motivo tiene que decir **donde** se busco: un "no se encontro ONNX
        // Runtime" sin la ruta obliga a leer el codigo para saber que esperaba.
        let error = probe_runtime_with(Path::new("/no/existe/nada"), None).unwrap_err();
        assert!(error.contains("/no/existe/nada"), "{error}");
    }

    #[test]
    fn an_empty_environment_variable_is_not_a_path() {
        // Una variable definida a cadena vacia no puede convertirse en "carga la
        // biblioteca del directorio actual": se ignora y se busca al lado del
        // ejecutable.
        let error = probe_runtime_with(Path::new("/no/existe/nada"), Some("   ")).unwrap_err();
        assert!(error.contains("/no/existe/nada"), "{error}");
    }

    #[test]
    fn a_wrong_runtime_path_is_reported_with_the_path_and_the_reason() {
        let error =
            probe_runtime_with(Path::new("/no/existe/nada"), Some("/tmp/no-es-orto.so"))
                .unwrap_err();

        assert!(error.contains("/tmp/no-es-orto.so"), "{error}");
        assert!(error.contains("ORT_DYLIB_PATH"), "{error}");
    }

    #[test]
    fn the_provider_announces_the_execution_provider_it_will_use() {
        // El nombre acaba en el informe del trabajo y `uses_vram` en `EvalVars`.
        // Los dos tienen que salir del EP configurado, no de lo que el hardware
        // recomendaria: son cosas distintas en cuanto se fija el EP a mano.
        let registry = Arc::new(
            ModelRegistry::from_json(r#"{"manifestVersion":2,"models":[]}"#).expect("manifiesto"),
        );

        let gpu = OrtProvider::new(
            PathBuf::from("/modelos"),
            Arc::clone(&registry),
            ProviderKind::Cuda,
            0,
            4,
        );
        assert_eq!(gpu.name(), "CUDA");
        assert!(gpu.uses_vram());

        let cpu = OrtProvider::new(
            PathBuf::from("/modelos"),
            registry,
            ProviderKind::Cpu,
            0,
            4,
        );
        assert_eq!(cpu.name(), "CPU");
        assert!(
            !cpu.uses_vram(),
            "la CPU no tiene VRAM que presupuestar"
        );
    }
}
