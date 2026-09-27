//! # su-cli
//!
//! Interfaz de linea de comandos del sidecar.
//!
//! Existe por dos razones concretas, no por completitud:
//!
//! 1. **Verificar el sidecar sin la interfaz.** El criterio de salida de la Fase 2
//!    es que `su-cli` escale una imagen; con esto se puede comprobar de verdad.
//! 2. **Integracion continua.** El harness de benchmark y los tests de humo
//!    necesitan un binario que haga el trabajo completo y devuelva un codigo de
//!    salida util.
//!
//! El comando `upscale` crea un trabajo real en la cola y se suscribe a su flujo
//! de eventos, asi que ejercita exactamente el mismo camino que usara la interfaz.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use su_core::{
    DenoiseChoice, FaceRestoreChoice, JobOptions, JobRequest, Mode, OutputFormat, OutputSettings,
    PipelineSet, Scale, SuResult, TileChoice,
};
use su_hardware::{probe, Capabilities};
use su_inference::RunnerConfig;
// Solo hace falta para nombrar el tipo del proveedor forzado a CPU, que sin la
// feature `onnx` no existe.
#[cfg(feature = "onnx")]
use su_inference::BackendProvider;
use su_jobs::{Event, JobContext, JobManager, JobStore};
use su_models::ModelRegistry;

#[derive(Parser)]
#[command(
    name = "su-cli",
    about = "Sidecar de SuperUpscaly: escalado de imagenes con aceleracion por hardware",
    version
)]
struct Cli {
    /// Directorio de datos (modelos, logs, cache).
    #[arg(long, global = true, default_value = "~/.local/share/superupscaly")]
    data_dir: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Escala una o varias imagenes.
    Upscale(UpscaleArgs),

    /// Muestra el hardware detectado y los execution providers disponibles.
    Capabilities,

    /// Lista los modelos del manifiesto y su estado en el cache local.
    Models(ModelsArgs),

    /// Muestra los pipelines disponibles y las etapas de cada uno.
    Pipelines,

    /// Arranca el servidor local.
    Serve(ServeArgs),
}

#[derive(clap::Args)]
struct UpscaleArgs {
    /// Imagenes de entrada. Se admiten varias.
    #[arg(required = true)]
    input: Vec<PathBuf>,

    /// Carpeta de salida.
    #[arg(short, long)]
    output: PathBuf,

    /// Modo de escalado.
    ///
    /// Se usa `default_value` (cadena) y no `default_value_t`: clap exige
    /// `Display` para la variante con valor por defecto tipado, y `ValueEnum` no
    /// lo aporta.
    #[arg(short, long, value_enum, default_value = "photo")]
    mode: ModeArg,

    /// Factor de escala.
    #[arg(short, long, default_value_t = 4)]
    scale: u32,

    /// Formato de salida.
    #[arg(long, value_enum, default_value = "png")]
    format: FormatArg,

    /// Calidad para formatos con perdida (1-100).
    #[arg(long, default_value_t = 95)]
    quality: u8,

    /// Tamano de tile. `auto` lo calcula a partir de la VRAM disponible.
    #[arg(long, default_value = "auto")]
    tile: String,

    /// Sufijo que se anade al nombre del archivo.
    #[arg(long, default_value = "_upscaled")]
    suffix: String,

    /// Reduce el ruido antes de escalar.
    #[arg(long)]
    denoise: bool,

    /// Aplica enfoque al final.
    #[arg(long)]
    sharpen: bool,
}

#[derive(clap::Args)]
struct ModelsArgs {
    /// Directorio donde se buscan los modelos.
    #[arg(long)]
    models_dir: Option<PathBuf>,

    /// Manifiesto a usar. Si no se indica, se usa el catalogo embebido.
    #[arg(long)]
    manifest: Option<PathBuf>,
}

#[derive(clap::Args)]
struct ServeArgs {
    /// Puerto. 0 deja que lo elija el sistema.
    #[arg(long, default_value_t = 0)]
    port: u16,

    /// Token de autenticacion. Si no se indica, se toma de `SU_TOKEN` y, si
    /// tampoco esta, se genera uno aleatorio.
    ///
    /// La aplicacion de escritorio lo pasa por la variable de entorno, no por
    /// aqui: los argumentos de un proceso los puede leer cualquier otro proceso
    /// del equipo. Ver `resolve_token`.
    #[arg(long)]
    token: Option<String>,

    /// Archivo donde publicar el puerto elegido.
    #[arg(long)]
    portfile: Option<PathBuf>,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ModeArg {
    Photo,
    Illustration,
}

impl From<ModeArg> for Mode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Photo => Mode::Photo,
            ModeArg::Illustration => Mode::Illustration,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum FormatArg {
    Png,
    Jpg,
    Webp,
}

impl From<FormatArg> for OutputFormat {
    fn from(value: FormatArg) -> Self {
        match value {
            FormatArg::Png => OutputFormat::Png,
            FormatArg::Jpg => OutputFormat::Jpg,
            FormatArg::Webp => OutputFormat::Webp,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let data_dir = expand_home(&cli.data_dir);

    // El registro se instala antes que nada, para que los avisos del arranque
    // queden en el archivo y no solo en la consola: cuando algo falla en segundo
    // plano, el archivo es lo unico que queda.
    //
    // Es **local**: escribe en el directorio de datos del usuario y enmascara su
    // ruta personal. No se envia nada a ningun sitio.
    let home = home_dir();
    match su_telemetry::init(
        &data_dir.join("logs"),
        home.as_deref().and_then(|path| path.to_str()).unwrap_or(""),
        &std::env::var("SU_LOG").unwrap_or_else(|_| "info".to_string()),
    ) {
        Ok(path) => tracing::info!(log = %path.display(), version = env!("CARGO_PKG_VERSION"), "arranque"),
        // No poder escribir el log no puede impedir usar la herramienta: se avisa
        // por la salida de error y se sigue.
        Err(error) => eprintln!("aviso: sin registro en archivo ({error})"),
    }

    match cli.command {
        Command::Capabilities => capabilities(),
        Command::Models(args) => models(args, &data_dir),
        Command::Pipelines => pipelines(),
        Command::Upscale(args) => upscale(args, &data_dir).await,
        Command::Serve(args) => serve(args, &data_dir).await,
    }
}

/// Expande `~` al directorio personal. La biblioteca estandar no lo hace y
/// escribir una ruta absoluta en cada invocacion seria incomodo.
fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Directorio personal del usuario, si se puede averiguar.
///
/// Se usa para dos cosas: expandir `~` y **enmascarar** esa ruta en el registro,
/// que es lo que evita que un log compartido para un informe de error lleve
/// dentro el nombre de usuario y su arbol de carpetas.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn capabilities() -> ExitCode {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));

    let capabilities = probe(&executable_dir);

    println!("CPU       {}", capabilities.cpu.brand);
    println!(
        "          {} nucleos fisicos, {} logicos (se usaran {} hilos)",
        capabilities.cpu.physical_cores,
        capabilities.cpu.logical_cores,
        capabilities.cpu.inference_threads()
    );
    println!("RAM       {} MiB", capabilities.ram_total_mb);

    if capabilities.gpus.is_empty() {
        println!("GPU       no se detecto ninguna");
    } else {
        for gpu in &capabilities.gpus {
            println!(
                "GPU       {} ({:?}), {} MiB libres de {}",
                gpu.name, gpu.vendor, gpu.vram_free_mb, gpu.vram_total_mb
            );
        }
    }

    println!();
    println!("Execution providers:");
    for entry in &capabilities.providers {
        let mark = if entry.available { "si" } else { "no" };
        match &entry.reason {
            Some(reason) => println!("  [{mark}] {:<12} {reason}", entry.kind.as_str()),
            None => println!("  [{mark}] {}", entry.kind.as_str()),
        }
    }
    println!();
    println!("Recomendado: {}", capabilities.recommended.as_str());

    ExitCode::SUCCESS
}

fn models(args: ModelsArgs, data_dir: &Path) -> ExitCode {
    let models_dir = args.models_dir.unwrap_or_else(|| data_dir.join("models"));

    let registry = match load_registry(args.manifest.as_ref()) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("Directorio: {}", models_dir.display());
    println!();

    let statuses = registry.statuses(&models_dir);
    if statuses.is_empty() {
        println!("El manifiesto no declara ningun modelo.");
        return ExitCode::SUCCESS;
    }

    for status in statuses {
        println!(
            "{:<28} {:<12} x{:<2} {:?}",
            status.id,
            format!("{:?}", status.kind).to_lowercase(),
            status.scale,
            status.state
        );
        if let Some(warning) = &status.license_warning {
            println!("  aviso legal: {warning}");
        }
    }

    ExitCode::SUCCESS
}

fn pipelines() -> ExitCode {
    let set = match PipelineSet::embedded() {
        Ok(set) => set,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    for pipeline in &set.pipelines {
        println!("{}  ({} etapas)", pipeline.id, pipeline.stages.len());
        if !pipeline.description.is_empty() {
            println!("  {}", pipeline.description);
        }
        for stage in &pipeline.stages {
            let model = stage
                .model
                .as_deref()
                .map(|value| format!(" -> {value}"))
                .unwrap_or_default();
            let conditional = if stage.when.is_some() { " (condicional)" } else { "" };
            println!("  - {}{}{}", stage.id, model, conditional);
        }
        println!();
    }

    ExitCode::SUCCESS
}

async fn upscale(args: UpscaleArgs, data_dir: &Path) -> ExitCode {
    let pipeline_set = match PipelineSet::embedded() {
        Ok(set) => Arc::new(set),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let scale = match Scale::try_from(args.scale) {
        Ok(scale) => scale,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let tile_size = match parse_tile(&args.tile) {
        Ok(tile) => tile,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };

    // Comprobacion temprana: si la carpeta de salida no se puede crear, mejor
    // saberlo antes de procesar nada.
    if let Err(error) = std::fs::create_dir_all(&args.output) {
        eprintln!("error: no se pudo crear {}: {error}", args.output.display());
        return ExitCode::FAILURE;
    }

    let executable_dir = executable_dir();
    let capabilities = probe(&executable_dir);

    let items: Vec<String> = args
        .input
        .iter()
        .map(|path| path.display().to_string())
        .collect();

    let request = JobRequest {
        mode: args.mode.into(),
        scale,
        items: items.clone(),
        output: OutputSettings {
            dir: args.output.display().to_string(),
            format: args.format.into(),
            quality: args.quality,
            suffix: args.suffix.clone(),
            preserve_metadata: true,
            zip_output: false,
        },
        options: JobOptions {
            tile_size,
            denoise: if args.denoise {
                DenoiseChoice::On
            } else {
                DenoiseChoice::Auto
            },
            sharpen: args.sharpen,
            face_restore: if Mode::from(args.mode) == Mode::Photo {
                FaceRestoreChoice::Auto
            } else {
                FaceRestoreChoice::Off
            },
            ..JobOptions::default()
        },
        priority: 0,
    };

    let registry = match load_registry(None) {
        Ok(registry) => Arc::new(registry),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let chosen = choose_provider(
        data_dir.join("models"),
        Arc::clone(&registry),
        &capabilities,
        detect_engine(&executable_dir),
    );
    // El nombre y la VRAM se toman del proveedor, no de `capabilities`: el informe
    // tiene que decir el backend que se uso, no el que se recomendo.
    let provider = Arc::clone(&chosen.default);
    let provider_name = provider.name().to_string();
    let uses_vram = provider.uses_vram();
    // Sin modelos instalados el motor con ONNX no puede escalar nada, y en un
    // equipo recien instalado ese es el caso normal: el trabajo se termina con
    // interpolacion, marcado como degradado y diciendo por que.
    let fallback = chosen.classical_fallback(&provider);

    let context = JobContext {
        models_dir: data_dir.join("models"),
        registry,
        provider,
        fallback,
        pipelines: pipeline_set,
        runner: RunnerConfig {
            tile_choice: tile_size,
            budget: None,
            ..RunnerConfig::default()
        },
        provider_name,
        is_cpu: !uses_vram,
        free_vram_mb: if uses_vram {
            capabilities.best_vram_free_mb()
        } else {
            0
        },
        cores: capabilities.cpu.inference_threads(),
    };

    let manager = JobManager::new(512);
    let mut events = manager.subscribe();

    let job = match manager.create(request, context) {
        Ok(job) => job,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "Trabajo {}: {} imagen(es), modo {:?}, x{}",
        job.id,
        items.len(),
        Mode::from(args.mode),
        scale.factor()
    );
    println!();

    let mut failed = 0usize;
    let mut done = 0usize;

    loop {
        match events.recv().await {
            Ok(Event::ItemStarted { name, .. }) => {
                println!("  procesando {name}");
            }
            Ok(Event::ItemProgress { percent, stage, .. }) => {
                print!("\r    {:<12} {:>3}%", format!("{stage:?}").to_lowercase(), (percent * 100.0) as u32);
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            Ok(Event::ItemCompleted {
                out_path,
                duration_ms,
                degraded,
                ..
            }) => {
                let flag = if degraded { " (degradado)" } else { "" };
                println!("\r    -> {out_path} en {duration_ms} ms{flag}          ");
                done += 1;
            }
            Ok(Event::ItemFailed { code, detail, .. }) => {
                println!("\r    fallo {code}: {detail}");
                failed += 1;
            }
            Ok(Event::JobFinished { status, .. }) => {
                println!();
                println!("Terminado: {status:?} ({done} correctas, {failed} fallidas)");
                return if failed == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                };
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                eprintln!("error: el flujo de eventos se cerro inesperadamente");
                return ExitCode::FAILURE;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
        }
    }
}

async fn serve(args: ServeArgs, data_dir: &Path) -> ExitCode {
    let token = resolve_token(args.token);

    let executable_dir = executable_dir();
    let capabilities = probe(&executable_dir);

    let registry = match load_registry(None) {
        Ok(registry) => Arc::new(registry),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let pipelines = match PipelineSet::embedded() {
        Ok(set) => Arc::new(set),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let models_dir = data_dir.join("models");

    // El proveedor se construye antes de ceder `capabilities` y `registry` a
    // `AppState`, que se queda con ellos.
    let chosen = choose_provider(
        models_dir.clone(),
        Arc::clone(&registry),
        &capabilities,
        detect_engine(&executable_dir),
    );

    // Persistencia: si no se puede abrir la base de datos, el servidor arranca
    // igual pero sin memoria entre sesiones. Es mejor eso que no arrancar.
    let jobs = match JobStore::open(&data_dir.join("jobs.db")) {
        Ok(store) => {
            let manager = JobManager::with_store(store, 1024);
            match manager.restore() {
                Ok(interrupted) if interrupted > 0 => {
                    eprintln!("aviso: {interrupted} trabajo(s) interrumpido(s) quedaron en pausa");
                }
                Ok(_) => {}
                Err(error) => eprintln!("aviso: no se pudieron restaurar los trabajos: {error}"),
            }
            manager
        }
        Err(error) => {
            eprintln!("aviso: sin persistencia de trabajos ({error})");
            JobManager::new(1024)
        }
    };

    let state = Arc::new(su_server::AppState::new(
        token,
        capabilities,
        registry,
        pipelines,
        models_dir,
        chosen,
        RunnerConfig::default(),
        jobs,
    ));

    let config = su_server::ServerConfig {
        port: args.port,
        portfile: args.portfile,
    };

    match su_server::serve(state, config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_tile(value: &str) -> Result<TileChoice, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(TileChoice::Auto),
        "256" => Ok(TileChoice::Px256),
        "384" => Ok(TileChoice::Px384),
        "512" => Ok(TileChoice::Px512),
        "768" => Ok(TileChoice::Px768),
        "1024" => Ok(TileChoice::Px1024),
        other => Err(format!(
            "tile '{other}' no valido (auto, 256, 384, 512, 768 o 1024)"
        )),
    }
}

/// Elige los proveedores de backends segun la compilacion.
///
/// Devuelve el de por defecto (el que corresponde al hardware) y, cuando tiene
/// sentido, uno forzado a CPU: el trabajo elige entre ellos con `DeviceChoice`.
/// Sin la feature `onnx` el de por defecto ya es el de referencia, que corre en
/// CPU, asi que no hay nada que forzar y el segundo es `None`.
/// Motor de inferencia que este binario puede usar en esta maquina.
///
/// Se decide una vez, al arrancar, y se pasa a [`choose_provider`] como dato: asi
/// la decision es una funcion comprobable en vez de una comprobacion escondida en
/// medio de la construccion de los proveedores.
enum InferenceEngine {
    /// ONNX Runtime cargado y listo para inferir.
    #[cfg(feature = "onnx")]
    Onnx { library: PathBuf },
    /// Sin runtime: interpolacion clasica, y el motivo por el que no lo hay.
    Classical { reason: String },
}

/// Decide el motor disponible: ONNX Runtime si se puede cargar, clasico si no.
fn detect_engine(executable_dir: &Path) -> InferenceEngine {
    #[cfg(feature = "onnx")]
    {
        match su_inference::probe_runtime(executable_dir) {
            Ok(library) => InferenceEngine::Onnx { library },
            Err(reason) => InferenceEngine::Classical { reason },
        }
    }

    #[cfg(not(feature = "onnx"))]
    {
        let _ = executable_dir;
        InferenceEngine::Classical {
            reason: "este binario se compilo sin la feature 'onnx'".to_string(),
        }
    }
}

fn choose_provider(
    models_dir: PathBuf,
    registry: Arc<ModelRegistry>,
    capabilities: &Capabilities,
    engine: InferenceEngine,
) -> su_server::Providers {
    #[cfg(feature = "onnx")]
    {
        // El runtime se comprueba **antes** de aceptar ningun trabajo. Un binario
        // compilado con ONNX pero sin la biblioteca al lado no puede inferir, y
        // descubrirlo al procesar la primera imagen significa aceptar un trabajo y
        // fallarlo despues. Si no hay runtime, se dice y se usa el motor clasico:
        // un interpolador de verdad da un resultado peor que un modelo, pero
        // muchisimo mejor que un error, y el informe dice cual de los dos corrio.
        let library = match engine {
            InferenceEngine::Onnx { library } => library,
            InferenceEngine::Classical { reason } => {
                tracing::warn!(%reason, "sin ONNX Runtime: motor clasico");
                eprintln!("aviso: {reason}");
                eprintln!(
                    "aviso: se usara interpolacion clasica en lugar de modelos de IA; el informe de cada imagen lo dira"
                );
                return classical_provider();
            }
        };

        tracing::info!(library = %library.display(), "ONNX Runtime cargado");

        let threads = capabilities.cpu.inference_threads();

        let default: Arc<dyn BackendProvider> = Arc::new(su_inference::OrtProvider::new(
            models_dir.clone(),
            Arc::clone(&registry),
            capabilities.recommended,
            // Indice de dispositivo: la primera GPU. El CLI todavia no permite
            // elegir otra.
            0,
            // ADR-014: en CPU se usan los nucleos fisicos.
            threads,
        ));

        // Solo hay algo que forzar si el de por defecto va a usar la GPU: si ya
        // corre en CPU, construir un segundo proveedor identico no aporta nada.
        let cpu = if default.uses_vram() {
            Some(Arc::new(su_inference::OrtProvider::new(
                models_dir,
                registry,
                su_tiling::ProviderKind::Cpu,
                0,
                threads,
            )) as Arc<dyn BackendProvider>)
        } else {
            None
        };

        su_server::Providers::new(default, cpu)
    }

    #[cfg(not(feature = "onnx"))]
    {
        // Sin la feature no hay modelo posible: este binario no sabe hablar con
        // ONNX Runtime. El directorio de modelos, el registro y las capacidades se
        // reciben igual para que la firma sea identica en las dos configuraciones.
        let _ = (models_dir, registry, capabilities);
        // Sin la feature `onnx` la unica variante posible es esta, asi que el
        // `let` no necesita condicion. Con `if let` el compilador avisa de que el
        // patron es irrefutable.
        let InferenceEngine::Classical { reason } = &engine;
        tracing::info!(%reason, "motor clasico");
        classical_provider()
    }
}

/// Motor clasico: interpolacion de alta calidad en lugar de modelos.
///
/// Es el respaldo de los dos casos en los que no hay inferencia posible: sin la
/// feature `onnx` y con la feature pero sin el runtime cargable. Sustituye a
/// `MockBackendProvider`, que era vecino mas cercano: en un x4 cada pixel salia
/// como un bloque de 4x4 y, peor, las etapas de restauracion del pipeline
/// (suben a x4 y vuelven a bajar) no reconstruian nada y devolvian la imagen
/// lavada, con los negros grises y los bordes emborronados.
fn classical_provider() -> su_server::Providers {
    su_server::Providers::new(Arc::new(su_inference::ClassicalProvider::default()), None)
}

/// Directorio del ejecutable.
///
/// Es donde se buscan los execution providers y la biblioteca de ONNX Runtime
/// (docs/06): el runtime se carga por nombre desde ahi, de modo que la misma
/// compilacion sirve para CPU y para GPU segun lo que se copie al lado.
fn executable_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Carga el manifiesto indicado o, si no lo hay, el catalogo embebido.
///
/// El catalogo embebido permite que `su-cli` funcione recien compilado, sin
/// ningun archivo de configuracion al lado.
fn load_registry(manifest: Option<&PathBuf>) -> SuResult<ModelRegistry> {
    match manifest {
        Some(path) => ModelRegistry::from_file(path),
        None => ModelRegistry::from_json(EMBEDDED_MANIFEST),
    }
}

/// Catalogo embebido.
///
/// Los modelos son pesos de terceros: el proyecto **no los redistribuye**, solo
/// apunta a la fuente original y los descarga en el primer uso. Por eso cada
/// entrada que se puede descargar lleva URL, `sha256` y tamano exactos: sin hash
/// no se ofrece descarga (ver `ModelStatus::download_for`), porque no habria
/// forma de saber si el archivo que llego es el modelo que dice ser.
///
/// Los hashes son los que publica el propio alojamiento (el `lfs.oid` de
/// HuggingFace) y se comprobaron contra el `content-length` del archivo. Si
/// alguno deja de coincidir, es que el archivo cambio en origen: hay que
/// revisarlo, no relajar la comprobacion.
///
/// `scunet-color` no lleva URL a proposito. Los unicos exports a ONNX que existen
/// reparten los pesos entre dos archivos (`.onnx` + `.onnx.data`) y el descargador
/// trae un archivo suelto. Se deja declarado para poder instalarlo a mano con
/// `localPath`, que es lo que ya soporta el manifiesto.
///
/// Aviso de licencias: `4x-ultrasharp` y `2x-animesharpv3` son
/// **CC-BY-NC-SA-4.0**, que no permite uso comercial. Declararlo no es cosmetico:
/// `license_warning()` lo convierte en un aviso visible antes de descargar.
const EMBEDDED_MANIFEST: &str = r#"{
  "manifestVersion": 2,
  "models": [
    { "id": "4x-ultrasharp", "name": "4x UltraSharp", "kind": "photo", "scale": 4,
      "autoPriority": 1,
      "sha256": "7295b39b71f1d5882fec1ae02f55227f7ca6516f92eae6920ab2a28a39cade73",
      "sizeBytes": 33605809,
      "urls": ["https://huggingface.co/Kim2091/UltraSharp/resolve/main/ONNX/4x-UltraSharp-fp16-opset17.onnx"],
      "license": { "name": "CC-BY-NC-SA-4.0", "commercialUse": false,
                   "note": "No permite uso comercial" } },
    { "id": "realesrgan-x4plus", "name": "Real-ESRGAN x4plus", "kind": "photo", "scale": 4,
      "autoPriority": 2,
      "sha256": "3767c17388381cfca3d7196a4a517737fefc22b57c9fd98d3bae78e98e2bebc9",
      "sizeBytes": 68811310,
      "urls": ["https://huggingface.co/mhmtaufiq/realesrgan-onnx/resolve/main/RealESRGAN_x4plus.onnx"],
      "license": { "name": "BSD-3-Clause", "commercialUse": true } },
    { "id": "realesrgan-x4plus-anime-6b", "name": "RealESRGAN x4plus Anime 6B",
      "kind": "illustration", "scale": 4, "autoPriority": 1,
      "sha256": "82f458db35dd94f2200f9a32fd0232c581da861c1b2e5c956d5574b0e9c5aea2",
      "sizeBytes": 18406820,
      "urls": ["https://huggingface.co/mhmtaufiq/realesrgan-onnx/resolve/main/RealESRGAN_x4plus_anime_6B.onnx"],
      "license": { "name": "BSD-3-Clause", "commercialUse": true } },
    { "id": "2x-animesharpv3", "name": "2x AnimeSharp V3", "kind": "illustration", "scale": 2,
      "sha256": "fe4cbe50bfc8b20dfcb16b0935ef4dbdb64547224bee17ec2f496385bc37a71e",
      "sizeBytes": 33619368,
      "urls": ["https://huggingface.co/Kim2091/AnimeSharpV3/resolve/main/2x-AnimeSharpV3-fp16.onnx"],
      "license": { "name": "CC-BY-NC-SA-4.0", "commercialUse": false,
                   "note": "No permite uso comercial" } },
    { "id": "scunet-color", "name": "SCUNet Color", "kind": "denoise", "scale": 1,
      "license": { "name": "Apache-2.0", "commercialUse": true },
      "notes": "Sin descarga: los exports a ONNX reparten los pesos en dos archivos. Instalar a mano con localPath." },
    { "id": "gfpgan-v1.4", "name": "GFPGAN v1.4", "kind": "face", "scale": 1,
      "sha256": "6548e54cbcf248af385248f0c1193b359c37a0f98b836282b09cf48af4fd2b73",
      "sizeBytes": 340256690,
      "urls": ["https://huggingface.co/netrunner-exe/Face-Upscalers-onnx/resolve/main/GFPGANv1.4.onnx"],
      "license": { "name": "Apache-2.0", "commercialUse": true } }
  ]
}"#;

/// Token de la API: el argumento manda, luego el entorno, luego uno nuevo.
fn resolve_token(from_args: Option<String>) -> String {
    choose_token(from_args, std::env::var("SU_TOKEN").ok()).unwrap_or_else(generate_token)
}

/// Elige entre el token recibido por argumento y el del entorno, en ese orden.
///
/// Se separa de la lectura del entorno para poder comprobarlo sin tocar las
/// variables del proceso, que en los tests son compartidas por todos los hilos.
///
/// Un valor vacio **no es un token**. Aceptarlo dejaria la API local abierta a
/// cualquiera que alcanzara el puerto, y una variable de entorno sin definir —o
/// definida a cadena vacia por un script— no puede degradar la seguridad en
/// silencio. Es justo el tipo de fallo que el proyecto existe para evitar.
fn choose_token(from_args: Option<String>, from_env: Option<String>) -> Option<String> {
    [from_args, from_env]
        .into_iter()
        .flatten()
        .find(|token| !token.trim().is_empty())
}

/// Token aleatorio de 32 bytes en hexadecimal.
///
/// Se genera con el reloj del sistema y una fuente del propio sistema operativo
/// cuando esta disponible. El token solo protege una API de loopback, asi que no
/// necesita ser criptograficamente perfecto, pero si impredecible.
fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);

    // xorshift64*: rapido y suficientemente bueno para un token de sesion.
    let mut out = String::with_capacity(64);
    for _ in 0..8 {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let value = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        out.push_str(&format!("{value:016x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use su_core::StageOp;

    /// Maquina con una GPU potente, que es el caso en el que equivocarse hace dano.
    fn machine_with_a_gpu() -> Capabilities {
        Capabilities {
            cpu: su_hardware::CpuInfo {
                brand: "prueba".to_string(),
                physical_cores: 8,
                logical_cores: 16,
            },
            gpus: vec![su_hardware::GpuInfo {
                name: "GPU de prueba".to_string(),
                vendor: su_hardware::GpuVendor::Nvidia,
                vram_total_mb: 24576,
                vram_free_mb: 20480,
                driver_version: None,
            }],
            ram_total_mb: 32768,
            providers: Vec::new(),
            recommended: su_tiling::ProviderKind::TensorRt,
            ort_version: None,
        }
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn without_onnx_the_provider_is_the_reference_one_even_on_a_gpu_machine() {
        // El informe y `EvalVars` tienen que describir el backend que se va a
        // ejecutar, no la maquina. Aqui `capabilities` recomienda TensorRT y hay
        // 20 GiB de VRAM libre, pero sin la feature `onnx` el backend real es el
        // de referencia, que corre en CPU. Si `is_cpu` y `free_vram_mb` se
        // leyeran de `capabilities`, el pipeline elegiria la rama de GPU y el
        // resultado anunciaria TensorRT sin haberlo usado.
        let registry = Arc::new(
            ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto embebido"),
        );

        let chosen = choose_provider(
            PathBuf::from("/modelos"),
            registry,
            &machine_with_a_gpu(),
            InferenceEngine::Classical {
                reason: "prueba".to_string(),
            },
        );

        // El nombre tiene que decir que no hubo modelo: "referencia" no lo decia, y
        // de ahi que nadie supiera si el resultado lo habia hecho una red o un
        // interpolador. Incluye el filtro, que es lo unico que distingue esta
        // salida de la de otro motor clasico.
        assert_eq!(chosen.default.name(), "clasico-catmullrom");
        assert!(
            !chosen.default.uses_vram(),
            "el motor clasico corre en CPU"
        );
        assert!(
            chosen.cpu.is_none(),
            "no hay nada que forzar cuando el de por defecto ya es de CPU"
        );
    }

    #[test]
    fn without_a_runtime_the_engine_says_why_and_where_it_looked() {
        let engine = detect_engine(Path::new("/no/existe/nada"));

        // `match` y no `let ... else` porque sin la feature `onnx` el `else` es
        // inalcanzable y el compilador lo avisa.
        let reason = match engine {
            InferenceEngine::Classical { reason } => reason,
            #[cfg(feature = "onnx")]
            InferenceEngine::Onnx { .. } => panic!("sin biblioteca no puede haber motor ONNX"),
        };

        // En la compilacion sin `onnx` el motivo es otro, pero en las dos el
        // mensaje tiene que servir para arreglarlo sin leer el codigo.
        assert!(!reason.is_empty());
        #[cfg(feature = "onnx")]
        assert!(reason.contains("/no/existe/nada"), "{reason}");
    }

    #[cfg(feature = "onnx")]
    #[test]
    fn with_a_loaded_runtime_the_provider_is_the_onnx_one() {
        let registry = Arc::new(
            ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto embebido"),
        );

        let chosen = choose_provider(
            PathBuf::from("/modelos"),
            registry,
            &machine_with_a_gpu(),
            InferenceEngine::Onnx {
                library: PathBuf::from("/tmp/libonnxruntime.so"),
            },
        );

        // Con ONNX disponible el motor es el de verdad, no el clasico.
        assert_ne!(chosen.default.name(), "clasico-lanczos3");
    }

    #[cfg(feature = "onnx")]
    #[test]
    fn with_onnx_the_provider_reports_the_recommended_execution_provider() {
        let registry = Arc::new(
            ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto embebido"),
        );

        let chosen = choose_provider(
            PathBuf::from("/modelos"),
            registry,
            &machine_with_a_gpu(),
            InferenceEngine::Onnx {
                library: PathBuf::from("/tmp/libonnxruntime.so"),
            },
        );

        assert_eq!(chosen.default.name(), "TensorRT");
        assert!(chosen.default.uses_vram());

        // Y existe la alternativa de CPU, que es lo que hace que el ajuste
        // "Dispositivo" de la interfaz signifique algo.
        let cpu = chosen
            .cpu
            .as_ref()
            .expect("deberia haber un proveedor forzado a CPU");
        assert_eq!(cpu.name(), "CPU");
        assert!(!cpu.uses_vram());
    }

    #[test]
    fn tile_parsing_accepts_the_documented_values() {
        assert_eq!(parse_tile("auto").unwrap(), TileChoice::Auto);
        assert_eq!(parse_tile("AUTO").unwrap(), TileChoice::Auto);
        assert_eq!(parse_tile("512").unwrap(), TileChoice::Px512);
        assert_eq!(parse_tile("1024").unwrap(), TileChoice::Px1024);
    }

    #[test]
    fn tile_parsing_rejects_anything_else_with_a_useful_message() {
        let error = parse_tile("999").unwrap_err();
        assert!(error.contains("999"));
        assert!(error.contains("auto"), "deberia listar los valores validos: {error}");
    }

    #[test]
    fn home_is_expanded() {
        // No se puede depender de que HOME exista, asi que se comprueba que una
        // ruta normal pasa intacta y que `~/` no deja el tilde si hay HOME.
        assert_eq!(expand_home("/tmp/x"), PathBuf::from("/tmp/x"));

        if std::env::var_os("HOME").is_some() || std::env::var_os("USERPROFILE").is_some() {
            let expanded = expand_home("~/modelos");
            assert!(!expanded.to_string_lossy().starts_with('~'));
            assert!(expanded.to_string_lossy().ends_with("modelos"));
        }
    }

    #[test]
    fn generated_tokens_are_long_and_differ_between_calls() {
        let first = generate_token();
        let second = generate_token();

        assert_eq!(first.len(), 128);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
        assert_ne!(first, second, "dos tokens seguidos no pueden coincidir");
    }

    #[test]
    fn the_argument_token_wins_over_the_environment() {
        assert_eq!(
            choose_token(Some("del-argumento".to_string()), Some("del-entorno".to_string())),
            Some("del-argumento".to_string())
        );
    }

    #[test]
    fn the_environment_token_is_used_when_there_is_no_argument() {
        assert_eq!(
            choose_token(None, Some("del-entorno".to_string())),
            Some("del-entorno".to_string())
        );
    }

    #[test]
    fn an_empty_token_is_not_a_token() {
        // Se prefiere generar uno nuevo a aceptar una cadena vacia: con un token
        // vacio, la API local quedaria abierta a cualquier proceso del equipo.
        assert_eq!(choose_token(Some(String::new()), None), None);
        assert_eq!(choose_token(Some("   ".to_string()), None), None);
        assert_eq!(choose_token(None, Some(String::new())), None);
        // Y un argumento vacio no tapa un entorno valido.
        assert_eq!(
            choose_token(Some(String::new()), Some("del-entorno".to_string())),
            Some("del-entorno".to_string())
        );
    }

    #[test]
    fn without_anything_a_token_is_generated() {
        assert_eq!(choose_token(None, None), None);
        assert_eq!(generate_token().len(), 128);
    }

    #[test]
    fn the_embedded_manifest_is_valid() {
        let registry = ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto embebido");
        assert!(registry.get("4x-ultrasharp").is_some());
        assert!(registry.get("realesrgan-x4plus-anime-6b").is_some());
    }

    #[test]
    fn every_downloadable_model_declares_a_verifiable_hash() {
        // Una URL sin hash es peor que no tener URL: la interfaz ofreceria una
        // descarga que no se puede comprobar, y el motor acabaria cargando un
        // archivo del que nadie sabe si es el modelo que dice ser. Un tamano a
        // cero deja el progreso a ciegas, y una URL sin TLS descarga pesos por un
        // canal que cualquiera puede reescribir.
        let registry = ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto");

        for model in registry.models() {
            if model.urls.is_empty() {
                continue;
            }

            let hash = model.sha256.as_deref().unwrap_or_else(|| {
                panic!("'{}' declara URL pero no sha256", model.id)
            });
            assert_eq!(
                hash.len(),
                64,
                "'{}' tiene un sha256 que no mide 64 caracteres",
                model.id
            );
            assert!(
                hash.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "'{}' tiene un sha256 que no es hexadecimal en minusculas",
                model.id
            );
            assert!(
                model.size_bytes > 0,
                "'{}' declara URL pero no tamano: el progreso seria indeterminado",
                model.id
            );
            for url in &model.urls {
                assert!(
                    url.starts_with("https://"),
                    "'{}' tiene una URL sin TLS: {url}",
                    model.id
                );
            }
        }
    }

    #[test]
    fn the_embedded_manifest_covers_every_stage_the_pipelines_need() {
        // Si un pipeline referencia un modelo que no esta en el catalogo, el
        // sidecar no puede ejecutarlo y el fallo apareceria en tiempo de
        // ejecucion, con el usuario delante.
        let registry = ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto");
        let pipelines = PipelineSet::embedded().expect("pipelines");

        for pipeline in &pipelines.pipelines {
            for stage in &pipeline.stages {
                let Some(model_id) = stage.model.as_deref() else {
                    continue;
                };
                assert!(
                    registry.get(model_id).is_some(),
                    "el pipeline '{}' usa '{}', que no esta en el catalogo",
                    pipeline.id,
                    model_id
                );
            }
        }
    }

    #[test]
    fn a_model_stage_without_a_scale_must_use_a_model_that_does_not_scale() {
        // Cuando una etapa no declara `scaleOut`, el runner asume la escala nativa
        // del modelo. Una etapa de restauracion usa un modelo x4 para limpiar a
        // tamano original: si no declara `scaleOut: 1`, el runner la trata como un
        // escalado y la cadena sale multiplicada. `lineclean` hacia exactamente
        // eso, y `illustration:4x` producia 16x.
        //
        // El manifiesto es la unica fuente que conoce la escala nativa de cada
        // modelo, asi que la comprobacion vive aqui, donde se ven los dos. En
        // ejecucion el fallo tambien lo caza `ScaleNotReached`, pero esto lo
        // detecta antes de compilar los tests de nada.
        let registry = ModelRegistry::from_json(EMBEDDED_MANIFEST).expect("manifiesto");
        let pipelines = PipelineSet::embedded().expect("pipelines");

        for pipeline in &pipelines.pipelines {
            for stage in &pipeline.stages {
                if stage.op != StageOp::Model || stage.scale_out.is_some() {
                    continue;
                }
                let Some(model_id) = stage.model.as_deref() else {
                    continue;
                };
                let model = registry
                    .get(model_id)
                    .unwrap_or_else(|| panic!("'{model_id}' no esta en el catalogo"));

                assert_eq!(
                    model.scale, 1,
                    "la etapa '{}' del pipeline '{}' no declara 'scaleOut' y usa '{}', que escala x{}: \
                     el runner la trataria como un escalado y la cadena saldria multiplicada",
                    stage.id, pipeline.id, model_id, model.scale
                );
            }
        }
    }

    #[test]
    fn mode_and_format_conversions_match_the_domain() {
        assert_eq!(Mode::from(ModeArg::Photo), Mode::Photo);
        assert_eq!(Mode::from(ModeArg::Illustration), Mode::Illustration);
        assert_eq!(OutputFormat::from(FormatArg::Png), OutputFormat::Png);
        assert_eq!(OutputFormat::from(FormatArg::Jpg).extension(), "jpg");
        assert_eq!(OutputFormat::from(FormatArg::Webp).extension(), "webp");
    }
}
