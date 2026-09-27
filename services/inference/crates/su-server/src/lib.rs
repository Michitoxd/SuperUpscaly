//! # su-server
//!
//! API local del sidecar. Escucha **solo en `127.0.0.1`** y exige un token en
//! todas las rutas salvo `/v1/health` (ADR-003).
//!
//! ## Por que HTTP y no IPC de Electron
//!
//! Los pixeles nunca cruzan esta frontera: los mensajes son rutas y metadatos, de
//! unos pocos kilobytes. A cambio, el sidecar es probable con `curl`, lo usa
//! `su-cli` sin acoplarse a Electron y el harness de benchmark puede lanzarlo
//! solo. Un canal IPC no daria nada de eso.
//!
//! ## Decisiones de seguridad
//!
//! - Bind explicito a loopback. Nunca `0.0.0.0`.
//! - El token se compara en tiempo constante: una comparacion normal filtraria
//!   informacion por el tiempo de respuesta.
//! - Se rechaza cualquier peticion cuyo cabecera `Host` no sea de loopback, que
//!   es la defensa contra *DNS rebinding* desde un navegador local.
//! - CORS deshabilitado: no se responde a preflight.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{info, warn};

use su_core::{DeviceChoice, Job, JobRequest, PipelineSet, SuError, SuResult};
use su_hardware::Capabilities;
use su_jobs::{ControlOutcome, JobContext, JobManager};
use su_models::{ModelRegistry, ModelStatus};

/// Version del protocolo. La interfaz comprueba que coincide antes de usar la API.
pub const PROTOCOL_VERSION: u32 = 1;

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Configuracion del servidor.
///
/// No incluye el token a proposito: vive en [`AppState`], que es quien lo
/// comprueba. Duplicarlo aqui invitaba a tener dos valores distintos y a validar
/// contra el equivocado.
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    /// Puerto. `0` deja que el sistema operativo elija uno libre.
    pub port: u16,
    /// Donde escribir el puerto elegido para que Electron lo encuentre.
    pub portfile: Option<PathBuf>,
}

/// Estado compartido por los manejadores.
pub struct AppState {
    token: String,
    capabilities: Capabilities,
    registry: Arc<ModelRegistry>,
    pipelines: Arc<PipelineSet>,
    models_dir: PathBuf,
    jobs: JobManager,
    shutdown: tokio::sync::Notify,
    /// Plantilla de contexto de trabajo, sin el directorio de modelos resuelto.
    job_template: JobTemplate,
}

struct JobTemplate {
    providers: Providers,
    runner: su_inference::RunnerConfig,
}

/// Los proveedores de backends entre los que un trabajo puede elegir.
///
/// Van juntos porque `DeviceChoice::Cpu` elige entre ellos, y por separado se
/// podria pasar uno sin el otro. Construirlos es responsabilidad de quien
/// arranca el servidor, que es quien tiene el directorio de modelos y el
/// registro para hacerlo.
pub struct Providers {
    /// El que corresponde al hardware (o el de referencia si no hay ONNX).
    pub default: Arc<dyn su_inference::BackendProvider>,
    /// El forzado a CPU, para cuando el trabajo pide CPU.
    ///
    /// `None` cuando el de por defecto ya corre en CPU: no hay nada que forzar.
    pub cpu: Option<Arc<dyn su_inference::BackendProvider>>,
    /// Interpolacion clasica, como respaldo del elegido.
    ///
    /// Existe porque el caso mas probable en una instalacion nueva es que el motor
    /// con ONNX arranque bien y **no haya ningun modelo descargado todavia**: sin
    /// respaldo, la primera imagen que el usuario prueba falla, que es justo lo que
    /// no debe pasar cuando la aplicacion puede hacer un trabajo peor pero real.
    /// Ver `docs/03-decisiones-adr.md` (ADR-025).
    pub classical: Arc<dyn su_inference::BackendProvider>,
}

impl Providers {
    /// Construye el trio de proveedores con la interpolacion clasica detras.
    ///
    /// Es el unico sitio donde se decide cual es el respaldo, para que el CLI y el
    /// servidor no puedan discrepar.
    pub fn new(
        default: Arc<dyn su_inference::BackendProvider>,
        cpu: Option<Arc<dyn su_inference::BackendProvider>>,
    ) -> Self {
        Self {
            default,
            cpu,
            classical: Arc::new(su_inference::ClassicalProvider::default()),
        }
    }

    /// El respaldo que aporta algo para el proveedor elegido.
    ///
    /// `None` si el elegido **ya es** el clasico: reintentar contra el mismo motor
    /// no arregla nada y solo duplicaria el trabajo.
    pub fn classical_fallback(
        &self,
        provider: &Arc<dyn su_inference::BackendProvider>,
    ) -> Option<Arc<dyn su_inference::BackendProvider>> {
        if Arc::ptr_eq(provider, &self.classical) {
            None
        } else {
            Some(Arc::clone(&self.classical))
        }
    }

    /// El proveedor que corresponde a la preferencia del trabajo.
    ///
    /// Pedir GPU en un equipo sin GPU no es un error: se usa el de por defecto y
    /// el informe del trabajo publica el proveedor real, asi que la interfaz dice
    /// lo que paso en lugar de callarse.
    fn for_device(&self, device: DeviceChoice) -> &Arc<dyn su_inference::BackendProvider> {
        match device {
            DeviceChoice::Cpu => self.cpu.as_ref().unwrap_or(&self.default),
            DeviceChoice::Auto | DeviceChoice::Gpu => &self.default,
        }
    }
}

impl AppState {
    // Ocho parametros: son las piezas del estado, no un conjunto de opciones. Cada
    // una viene de un sitio distinto del arranque y agruparlas en una estructura
    // intermedia solo anadiria un tipo que nadie mas usa.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        token: impl Into<String>,
        capabilities: Capabilities,
        registry: Arc<ModelRegistry>,
        pipelines: Arc<PipelineSet>,
        models_dir: PathBuf,
        providers: Providers,
        runner: su_inference::RunnerConfig,
        jobs: JobManager,
    ) -> Self {
        Self {
            token: token.into(),
            capabilities,
            registry,
            pipelines,
            models_dir,
            jobs,
            shutdown: tokio::sync::Notify::new(),
            job_template: JobTemplate { providers, runner },
        }
    }

    /// Contexto de ejecucion de un trabajo nuevo.
    ///
    /// `device` sale de las opciones del trabajo. Antes este ajuste no se leia en
    /// ninguna parte del workspace, asi que el control "Dispositivo" de la
    /// interfaz no hacia absolutamente nada.
    fn job_context(&self, device: DeviceChoice) -> JobContext {
        // El proveedor es la unica fuente de verdad sobre el backend que se va a
        // usar. `capabilities` describe la maquina, no lo que se ha compilado: en
        // una maquina con TensorRT pero sin la feature `onnx` el backend real es
        // el de referencia, y anunciar TensorRT (o dar VRAM que nadie va a usar)
        // haria que las condiciones del pipeline eligieran una rama inexistente.
        let provider = self.job_template.providers.for_device(device);
        let uses_vram = provider.uses_vram();

        JobContext {
            models_dir: self.models_dir.clone(),
            registry: self.registry.clone(),
            provider: Arc::clone(provider),
            fallback: self.job_template.providers.classical_fallback(provider),
            pipelines: self.pipelines.clone(),
            runner: self.job_template.runner.clone(),
            provider_name: provider.name().to_string(),
            is_cpu: !uses_vram,
            free_vram_mb: if uses_vram {
                self.capabilities.best_vram_free_mb()
            } else {
                0
            },
            cores: self.capabilities.cpu.inference_threads(),
        }
    }

    pub fn jobs(&self) -> &JobManager {
        &self.jobs
    }

    pub fn shutdown_signal(&self) {
        // `notify_one` guarda un permiso si nadie esta esperando todavia. Con
        // `notify_waiters` el aviso se perderia si el cierre llega antes de que
        // el bucle de `serve` empiece a escuchar.
        self.shutdown.notify_one();
    }
}

// ---------------------------------------------------------------------------
// Errores de la API
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "SU-E401".to_string(),
            message: "falta el token o no es valido".to_string(),
        }
    }

    fn forbidden_host() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "SU-E403".to_string(),
            message: "la cabecera Host no apunta a loopback".to_string(),
        }
    }

    fn not_found(what: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "SU-E404".to_string(),
            message: format!("{what} no encontrado"),
        }
    }
}

impl From<SuError> for ApiError {
    fn from(error: SuError) -> Self {
        let status = match error.code().as_str() {
            "SU-E001" => StatusCode::BAD_REQUEST,
            "SU-E110" | "SU-E111" => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            code: error.code().as_str().to_string(),
            message: error.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    code: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

// ---------------------------------------------------------------------------
// Autenticacion
// ---------------------------------------------------------------------------

/// Comparacion en tiempo constante.
///
/// Una comparacion normal de cadenas termina en el primer byte distinto, asi que
/// el tiempo de respuesta revela cuantos caracteres del token son correctos. Con
/// 32 bytes y una API local el riesgo es bajo, pero el arreglo cuesta diez lineas.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

/// La cabecera `Host` debe apuntar a loopback. Sin esta comprobacion, una pagina
/// web abierta en el equipo podria resolver un dominio propio a 127.0.0.1 y
/// hablar con el sidecar (DNS rebinding).
fn host_is_loopback(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(axum::http::header::HOST) else {
        return true; // Sin cabecera Host no hay ataque posible.
    };
    let Ok(host) = host.to_str() else {
        return false;
    };

    let name = host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host);

    matches!(name, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

async fn authorize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiError> {
    if !host_is_loopback(&headers) {
        warn!("peticion rechazada: cabecera Host no local");
        return Err(ApiError::forbidden_host());
    }

    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();

    if !constant_time_eq(provided, &state.token) {
        return Err(ApiError::unauthorized());
    }

    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Rutas
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthBody {
    status: &'static str,
    version: &'static str,
    protocol: u32,
}

/// Liveness. Es la unica ruta sin autenticacion: Electron necesita poder
/// comprobar que el proceso esta vivo antes de tener nada mas.
async fn health() -> Json<HealthBody> {
    Json(HealthBody {
        status: "ok",
        version: SERVER_VERSION,
        protocol: PROTOCOL_VERSION,
    })
}

/// Respuesta de `GET /v1/capabilities`.
///
/// Es el informe de hardware **mas el motor que se va a usar de verdad**. Los dos
/// datos juntos y no solo el primero: una maquina con TensorRT describe lo que hay,
/// no lo que se ha compilado ni lo que se ha podido cargar. Enseñar unicamente
/// "TensorRT" cuando el sidecar va a interpolar en CPU es la forma mas facil de
/// que el usuario crea que ve calidad de modelo cuando no la hay.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitiesBody {
    #[serde(flatten)]
    hardware: Capabilities,
    /// Nombre del proveedor de backends por defecto: `CPU`, `CUDA`, o
    /// `clasico-<filtro>` cuando no hay ONNX Runtime o ningun modelo posible.
    engine: String,
}

async fn capabilities(State(state): State<Arc<AppState>>) -> Json<CapabilitiesBody> {
    Json(CapabilitiesBody {
        hardware: state.capabilities.clone(),
        engine: state.job_template.providers.default.name().to_string(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelsBody {
    /// Directorio donde el sidecar busca los modelos.
    ///
    /// Se expone para que la aplicacion descargue ahi en lugar de reproducir la
    /// logica de plataforma que decide la ruta. Si las dos divergieran, la
    /// interfaz diria "instalado" y el motor seguiria sin encontrar el archivo.
    models_dir: String,
    models: Vec<ModelStatus>,
}

async fn models(State(state): State<Arc<AppState>>) -> Json<ModelsBody> {
    Json(ModelsBody {
        models_dir: state.models_dir.display().to_string(),
        models: state.registry.statuses(&state.models_dir),
    })
}

async fn pipelines(State(state): State<Arc<AppState>>) -> Json<PipelineSet> {
    Json((*state.pipelines).clone())
}

async fn create_job(
    State(state): State<Arc<AppState>>,
    Json(request): Json<JobRequest>,
) -> Result<(StatusCode, Json<Job>), ApiError> {
    let device = request.options.device;
    let job = state.jobs.create(request, state.job_context(device))?;
    Ok((StatusCode::CREATED, Json(job)))
}

async fn list_jobs(State(state): State<Arc<AppState>>) -> Json<Vec<Job>> {
    Json(state.jobs.list())
}

async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    state
        .jobs
        .get(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("trabajo"))
}

/// Pausa un trabajo.
///
/// Distingue dos cosas que antes respondian lo mismo, y mal: un id que no existe
/// —un 404— de un trabajo que existe pero **ya no esta en curso**. El segundo caso
/// no es un fallo del motor: la peticion se ha quedado sin efecto porque no habia
/// nada que pausar. Antes respondia `500` con "no hay ningun trabajo activo", que
/// es lo que hacia que la interfaz anunciara un error del motor al pulsar pausa
/// unas decimas de segundo despues de que el trabajo terminara solo.
///
/// La comprobacion y la accion las hace el gestor en una sola operacion. Preguntar
/// aqui `is_alive` y pausar despues dejaba una ventana de milisegundos en la que el
/// trabajo terminaba entre ambas llamadas: la pausa lanzaba entonces un error
/// interno y volvia el 500 que esta funcion existe para evitar.
///
/// Se devuelve el trabajo tal y como esta. No se miente sobre el estado: si ya
/// termino, el cliente recibe que ha terminado.
async fn pause_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    if state.jobs.get(&id).is_none() {
        return Err(ApiError::not_found("trabajo"));
    }

    if state.jobs.pause(&id)? == ControlOutcome::NotRunning {
        tracing::debug!(job = %id, "pausa sin efecto: el trabajo no esta en curso");
    }

    state
        .jobs
        .get(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("trabajo"))
}

/// Reanuda un trabajo: lo despausa si sigue vivo, lo relanza si viene de una
/// sesion anterior. El cliente no deberia tener que distinguir los dos casos.
async fn resume_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    // La preferencia de dispositivo se recupera del propio trabajo, no del estado
    // del servidor: reanudar tiene que usar lo mismo que se eligio al crearlo.
    let device = state
        .jobs
        .get(&id)
        .map(|job| job.options.device)
        .unwrap_or(DeviceChoice::Auto);

    let job = state.jobs.resume_or_restart(&id, state.job_context(device))?;
    Ok(Json(job))
}

/// Cancela un trabajo. Vale el mismo razonamiento que en `pause_job`: cancelar
/// algo que ya termino no es un fallo del motor, es una peticion que llego tarde.
async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    if state.jobs.get(&id).is_none() {
        return Err(ApiError::not_found("trabajo"));
    }

    if state.jobs.cancel(&id)? == ControlOutcome::NotRunning {
        tracing::debug!(job = %id, "cancelacion sin efecto: el trabajo no esta en curso");
    }

    state
        .jobs
        .get(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("trabajo"))
}

async fn events(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_events(socket, state))
}

async fn stream_events(mut socket: WebSocket, state: Arc<AppState>) {
    let mut receiver = state.jobs.subscribe();

    loop {
        match receiver.recv().await {
            Ok(event) => {
                let Ok(text) = serde_json::to_string(&event) else {
                    continue;
                };
                if socket.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }

            // El cliente se quedo atras y perdio eventos intermedios. No es un
            // fallo: el estado real se puede recuperar con `GET /v1/jobs/{id}`.
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                warn!(skipped, "el cliente de eventos se quedo atras");
                continue;
            }

            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShutdownBody {
    status: &'static str,
}

async fn shutdown(State(state): State<Arc<AppState>>) -> Json<ShutdownBody> {
    info!("cierre solicitado por el cliente");
    state.shutdown_signal();
    Json(ShutdownBody { status: "closing" })
}

/// Construye el router. Se expone para poder montarlo en tests sin abrir un socket.
pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/models", get(models))
        .route("/v1/pipelines", get(pipelines))
        .route("/v1/jobs", post(create_job).get(list_jobs))
        // `:id` y no `{id}`: esta version de axum (0.7) marca los parametros de
        // ruta con dos puntos. Con llaves, el segmento se toma como texto literal
        // —solo coincidiria la URL "/v1/jobs/{id}"— y toda peticion con un id de
        // verdad responde 404. El sintoma es de los que cuestan: el trabajo se crea
        // bien y la interfaz se queda sin poder consultarlo, pausarlo ni cancelarlo.
        .route("/v1/jobs/:id", get(get_job).delete(cancel_job))
        .route("/v1/jobs/:id/pause", post(pause_job))
        .route("/v1/jobs/:id/resume", post(resume_job))
        .route("/v1/jobs/:id/cancel", post(cancel_job))
        .route("/v1/events", get(events))
        .route("/v1/shutdown", post(shutdown))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));

    Router::new()
        .route("/v1/health", get(health))
        .merge(protected)
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Arranque
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortFile {
    port: u16,
    pid: u32,
    version: String,
    protocol: u32,
    started_at: String,
}

/// Arranca el servidor y no regresa hasta que se cierra.
///
/// Escribe el *portfile* de forma atomica en cuanto conoce el puerto real, que es
/// lo que permite usar `--port 0` y dejar que el sistema elija.
pub async fn serve(state: Arc<AppState>, config: ServerConfig) -> SuResult<()> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), config.port);

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| SuError::Internal(format!("no se pudo escuchar en {address}: {error}")))?;

    let local = listener
        .local_addr()
        .map_err(|error| SuError::Internal(format!("sin direccion local: {error}")))?;

    if let Some(portfile) = config.portfile.as_ref() {
        let payload = PortFile {
            port: local.port(),
            pid: std::process::id(),
            version: SERVER_VERSION.to_string(),
            protocol: PROTOCOL_VERSION,
            started_at: su_jobs::now_iso8601(),
        };
        write_portfile(portfile, &payload)?;
    }

    info!(port = local.port(), "sidecar escuchando en loopback");

    let app = router(state.clone());

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            state.shutdown.notified().await;
        })
        .await
        .map_err(|error| SuError::Internal(format!("el servidor termino con error: {error}")))?;

    info!("sidecar detenido");
    Ok(())
}

fn write_portfile(path: &PathBuf, payload: &PortFile) -> SuResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            SuError::WriteFailed(format!("no se pudo crear {}: {error}", parent.display()))
        })?;
    }

    let json = serde_json::to_string_pretty(payload)
        .map_err(|error| SuError::Internal(format!("portfile no serializable: {error}")))?;

    // Atomico: Electron puede leer el archivo en cualquier momento y no debe
    // encontrarse un JSON a medias.
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, json)
        .map_err(|error| SuError::WriteFailed(format!("portfile: {error}")))?;
    std::fs::rename(&temporary, path)
        .map_err(|error| SuError::WriteFailed(format!("portfile: {error}")))?;

    Ok(())
}

/// Marca de error de una peticion sin token, para los tests.
pub fn unauthorized_status() -> StatusCode {
    StatusCode::UNAUTHORIZED
}

#[cfg(test)]
mod tests {
    use super::*;
    use su_hardware::{CpuInfo, ProviderAvailability, ProviderKind};
    use su_inference::MockBackendProvider;

    /// Proveedor de mentira que solo sabe identificarse.
    struct NamedProvider(&'static str, bool);

    impl su_inference::BackendProvider for NamedProvider {
        fn name(&self) -> &str {
            self.0
        }

        fn uses_vram(&self) -> bool {
            self.1
        }

        fn backend_for(&self, model_id: &str) -> SuResult<Box<dyn su_inference::Backend>> {
            MockBackendProvider.backend_for(model_id)
        }
    }

    #[test]
    fn a_job_that_asks_for_cpu_gets_the_cpu_provider() {
        // `DeviceChoice` no se leia en ninguna parte del workspace, asi que el
        // control "Dispositivo" de la interfaz no hacia absolutamente nada.
        let providers = Providers::new(
            Arc::new(NamedProvider("GPU", true)),
            Some(Arc::new(NamedProvider("CPU", false))),
        );

        assert_eq!(providers.for_device(DeviceChoice::Cpu).name(), "CPU");
        assert_eq!(providers.for_device(DeviceChoice::Auto).name(), "GPU");
        assert_eq!(providers.for_device(DeviceChoice::Gpu).name(), "GPU");
    }

    #[test]
    fn asking_for_cpu_without_a_separate_provider_uses_the_default() {
        // Sin la feature `onnx` el de por defecto ya corre en CPU y no hay segundo
        // proveedor. Pedir CPU tiene que seguir funcionando, no fallar.
        let providers = Providers::new(Arc::new(MockBackendProvider), None);

        let chosen = providers.for_device(DeviceChoice::Cpu);
        assert_eq!(chosen.name(), "referencia");
        assert!(!chosen.uses_vram());
    }

    fn state(token: &str) -> Arc<AppState> {
        let capabilities = Capabilities {
            cpu: CpuInfo {
                brand: "test".to_string(),
                physical_cores: 4,
                logical_cores: 8,
            },
            gpus: Vec::new(),
            ram_total_mb: 8192,
            providers: vec![ProviderAvailability {
                kind: ProviderKind::Cpu,
                available: true,
                reason: None,
            }],
            recommended: ProviderKind::Cpu,
            ort_version: None,
        };

        let registry = Arc::new(
            ModelRegistry::from_json(r#"{"manifestVersion":2,"models":[]}"#).expect("manifiesto"),
        );
        let pipelines = Arc::new(PipelineSet::embedded().expect("pipelines"));

        Arc::new(AppState::new(
            token,
            capabilities,
            registry,
            pipelines,
            std::env::temp_dir().join("su-server-models"),
            Providers::new(Arc::new(MockBackendProvider), None),
            su_inference::RunnerConfig::default(),
            JobManager::new(64),
        ))
    }

    #[test]
    fn tokens_are_compared_in_constant_time() {
        assert!(constant_time_eq("abc123", "abc123"));
        assert!(!constant_time_eq("abc123", "abc124"));
        assert!(!constant_time_eq("abc123", "abc"));
        assert!(!constant_time_eq("", "abc123"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn only_loopback_hosts_are_accepted() {
        let mut headers = HeaderMap::new();

        for host in ["127.0.0.1:51234", "localhost:8080", "127.0.0.1", "[::1]:9000"] {
            headers.insert(axum::http::header::HOST, host.parse().unwrap());
            assert!(host_is_loopback(&headers), "{host} deberia aceptarse");
        }

        for host in ["evil.example.com", "192.168.1.10:8080", "example.com:51234"] {
            headers.insert(axum::http::header::HOST, host.parse().unwrap());
            assert!(!host_is_loopback(&headers), "{host} deberia rechazarse");
        }
    }

    #[test]
    fn a_request_without_a_host_header_is_allowed() {
        // Sin cabecera Host no hay DNS rebinding posible, y algunos clientes
        // locales no la envian.
        assert!(host_is_loopback(&HeaderMap::new()));
    }

    #[test]
    fn the_error_response_carries_the_domain_code() {
        let error = ApiError::from(SuError::ModelMissing("4x-ultrasharp".to_string()));
        assert_eq!(error.code, "SU-E110");
        assert_eq!(error.status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn a_no_images_error_maps_to_400() {
        let error = ApiError::from(SuError::NoImagesFound);
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn the_portfile_is_written_atomically() {
        let dir = std::env::temp_dir().join(format!("su-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("runtime.json");

        let payload = PortFile {
            port: 51234,
            pid: 4321,
            version: "0.1.0".to_string(),
            protocol: PROTOCOL_VERSION,
            started_at: "2026-09-15T00:00:00Z".to_string(),
        };

        write_portfile(&path, &payload).expect("escritura");

        let raw = std::fs::read_to_string(&path).expect("lectura");
        let parsed: PortFile = serde_json::from_str(&raw).expect("json valido");
        assert_eq!(parsed.port, 51234);
        assert_eq!(parsed.protocol, PROTOCOL_VERSION);
        assert!(!dir.join("runtime.tmp").exists(), "quedo el temporal");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_router_builds_without_panicking() {
        // Comprobacion barata de que las rutas y el middleware encajan.
        let _ = router(state("token-de-prueba"));
    }

    #[test]
    fn protocol_version_is_one() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
