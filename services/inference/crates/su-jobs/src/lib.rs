//! # su-jobs
//!
//! Cola de trabajos y ejecucion por lotes.
//!
//! ## Decisiones
//!
//! - **Un hilo por trabajo, no una tarea de tokio.** El procesado de un item es
//!   trabajo de CPU puro y sin puntos de espera reales; meterlo en el runtime
//!   asincrono solo anadiria complejidad y riesgo de bloquear el reactor. El
//!   servidor HTTP sigue siendo asincrono: los trabajos viven fuera de el y se
//!   comunican por un canal de eventos.
//! - **La persistencia en SQLite llega en la Fase 3.** Aqui el estado vive en
//!   memoria, que es suficiente para el MVP y mantiene el crate sin dependencias
//!   de base de datos mientras se valida el flujo.
//! - **Cancelar y pausar son cooperativos**, y se comprueban **entre imagenes**.
//!   Nunca a mitad de una inferencia: parar en un punto seguro es lo que permite
//!   reanudar despues sin corromper nada.
//! - **Se persiste en cuatro momentos**, no en cada evento de progreso: al crear
//!   el trabajo, al terminar cada imagen y al cerrar el lote. Un lote con cientos
//!   de tiles generaria miles de escrituras por imagen si se guardara el progreso
//!   tile a tile. Perder el avance de la imagen en curso es asumible; perder el de
//!   las ya terminadas no.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use su_analyze::analyze;
use su_core::{
    Analysis, EvalVars, ItemStatus, Job, JobItem, JobOptions, JobProgress, JobRequest, JobStatus,
    Mode, OutputSettings, Pipeline, PipelineSet, Scale, StageId, SuError, SuResult,
};
use su_inference::{run_pipeline, BackendProvider, RunOutcome, RunnerConfig};
use su_models::ModelRegistry;
use tokio::sync::broadcast;

pub mod store;

pub use store::JobStore;

/// Suceso emitido durante la ejecucion de un trabajo.
///
/// Todos los variantes llevan `job_id` para que el cliente pueda filtrar sin
/// mantener estado. Los nombres de campo van en `camelCase` porque los consume
/// la interfaz de TypeScript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    JobCreated {
        #[serde(rename = "jobId")]
        job_id: String,
        total: usize,
    },
    JobStarted {
        #[serde(rename = "jobId")]
        job_id: String,
        total: usize,
    },
    JobProgress {
        #[serde(rename = "jobId")]
        job_id: String,
        done: usize,
        failed: usize,
        degraded: usize,
        total: usize,
    },
    JobPaused {
        #[serde(rename = "jobId")]
        job_id: String,
    },
    JobResumed {
        #[serde(rename = "jobId")]
        job_id: String,
    },
    JobFinished {
        #[serde(rename = "jobId")]
        job_id: String,
        status: JobStatus,
        done: usize,
        failed: usize,
        degraded: usize,
    },
    ItemStarted {
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "itemId")]
        item_id: String,
        name: String,
    },
    ItemProgress {
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "itemId")]
        item_id: String,
        stage: StageId,
        done: u32,
        total: u32,
        percent: f32,
    },
    ItemCompleted {
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "itemId")]
        item_id: String,
        #[serde(rename = "outPath")]
        out_path: String,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
        /// Etapas que se ejecutaron de verdad.
        executed: Vec<StageId>,
        /// Etapas omitidas, con el motivo.
        skipped: Vec<String>,
        degraded: bool,
    },
    ItemFailed {
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "itemId")]
        item_id: String,
        code: String,
        detail: String,
    },
    Warning {
        #[serde(rename = "jobId")]
        job_id: Option<String>,
        message: String,
    },
}

/// Estado de control de un trabajo en curso.
#[derive(Debug, Clone, Default)]
struct JobControl {
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

impl JobControl {
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn is_paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }
}

/// Resultado de una orden de control (pausar, reanudar, cancelar).
///
/// Existe porque hay dos cosas que no son lo mismo y que antes compartian un
/// unico error: un trabajo **que no existe** y un trabajo que existe pero **ya no
/// tiene hilo en ejecucion**. El segundo caso no es un fallo: pausar algo que
/// acaba de terminar solo significa que no habia nada que pausar. Distinguirlos
/// dentro del gestor es lo que cierra la ventana de carrera que quedaba al
/// preguntar `is_alive` desde fuera y actuar despues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOutcome {
    /// La orden llego al hilo que estaba ejecutando el trabajo.
    Applied,
    /// El trabajo no tiene hilo activo: la orden se ha quedado sin efecto.
    NotRunning,
}

/// Todo lo que hace falta para ejecutar un trabajo.
pub struct JobContext {
    pub models_dir: PathBuf,
    pub registry: Arc<ModelRegistry>,
    pub provider: Arc<dyn BackendProvider>,
    /// Motor al que recurrir cuando el elegido **no puede trabajar**: falta el
    /// modelo que la etapa necesita, el execution provider no arranca o el hash
    /// del archivo descargado no cuadra.
    ///
    /// Es el mismo criterio que ya se aplica cuando no hay ONNX Runtime en el
    /// equipo: un interpolador de verdad da un resultado peor que un modelo, pero
    /// muchisimo mejor que una imagen fallida. La diferencia es que aqui el motivo
    /// se anota en el informe del item, y el item queda marcado como degradado, de
    /// modo que "¿por que esta imagen salio distinta?" tiene respuesta.
    ///
    /// `None` cuando el proveedor elegido **ya es** el de respaldo (reintentar
    /// contra si mismo no arregla nada) o cuando quien construye el contexto no
    /// tiene ninguno. Con `None` el fallo se propaga, que es el comportamiento de
    /// antes.
    pub fallback: Option<Arc<dyn BackendProvider>>,
    pub pipelines: Arc<PipelineSet>,
    pub runner: RunnerConfig,
    /// EP elegido, para las variables de las condiciones.
    pub provider_name: String,
    pub is_cpu: bool,
    pub free_vram_mb: u64,
    pub cores: usize,
}

/// Cola de trabajos en memoria.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

struct Inner {
    jobs: RwLock<HashMap<String, Job>>,
    control: RwLock<HashMap<String, JobControl>>,
    events: broadcast::Sender<Event>,
    /// `None` si se trabaja sin persistencia (tests, `su-cli` en un solo paso).
    ///
    /// Va en un mutex porque `rusqlite::Connection` no es `Sync`. Se mantiene
    /// visible en lugar de esconderlo detras de una API que parezca libre de
    /// contencion.
    store: Option<Mutex<JobStore>>,
    /// Trabajos creados que aun no han empezado, con lo que necesitan para
    /// arrancar. Se guarda aqui en lugar de lanzarlos al crearlos para poder
    /// respetar el limite de concurrencia y el orden de prioridad.
    pending: Mutex<VecDeque<PendingJob>>,
    /// Cuantos trabajos se estan ejecutando ahora mismo.
    running: AtomicUsize,
    /// Cuantos pueden ejecutarse a la vez. Ver `set_max_concurrent`.
    max_concurrent: AtomicUsize,
}

/// Un trabajo esperando turno.
struct PendingJob {
    id: String,
    priority: u8,
    /// Numero de creacion. Desempata por orden de llegada, para que dos trabajos
    /// con la misma prioridad no se adelanten el uno al otro.
    sequence: u64,
    pipeline: Pipeline,
    control: JobControl,
    context: JobContext,
}

/// Techo del limite de concurrencia.
///
/// Ocho es el mismo techo que valida la frontera IPC. Por encima de eso no hay
/// GPU ni equipo de sobremesa que saque provecho: las inferencias se reparten la
/// misma memoria y el mismo ancho de banda.
const MAX_CONCURRENT: usize = 8;

/// Elige el indice del siguiente trabajo a arrancar.
///
/// Mas prioridad primero y, a igualdad, el que llego antes. Se aisla en una
/// funcion pura para poder comprobar la regla sin montar trabajos de verdad: es
/// justo la clase de detalle que se rompe en silencio cuando alguien toca la cola.
fn next_index(entries: &[(u8, u64)]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .max_by_key(|(_, (priority, sequence))| (*priority, std::cmp::Reverse(*sequence)))
        .map(|(index, _)| index)
}

/// Devuelve el turno al planificador al terminar el hilo de un trabajo.
///
/// Se hace en `Drop` y no con una llamada al final del bucle para que una salida
/// temprana o un `panic` no dejen el contador de trabajos en vuelo subido: con el
/// contador desajustado, la cola se quedaria parada para siempre y ningun trabajo
/// volveria a arrancar.
struct SlotGuard {
    manager: JobManager,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.manager.release_slot();
    }
}

impl JobManager {
    /// Gestor sin persistencia. Los trabajos viven solo mientras el proceso.
    pub fn new(event_capacity: usize) -> Self {
        Self::build(None, event_capacity)
    }

    /// Gestor con persistencia. Los trabajos sobreviven a un cierre inesperado.
    pub fn with_store(store: JobStore, event_capacity: usize) -> Self {
        Self::build(Some(store), event_capacity)
    }

    fn build(store: Option<JobStore>, event_capacity: usize) -> Self {
        let (events, _receiver) = broadcast::channel(event_capacity.max(16));
        Self {
            inner: Arc::new(Inner {
                jobs: RwLock::new(HashMap::new()),
                control: RwLock::new(HashMap::new()),
                events,
                store: store.map(Mutex::new),
                pending: Mutex::new(VecDeque::new()),
                running: AtomicUsize::new(0),
                // Una imagen a la vez por defecto, que es lo que recomienda el
                // plan: dos inferencias compitiendo por la misma GPU se estorban
                // mas de lo que se ayudan.
                max_concurrent: AtomicUsize::new(1),
            }),
        }
    }

    /// Cuantos trabajos pueden ejecutarse a la vez.
    ///
    /// El limite es **global**, no por trabajo: lo comparten todos los trabajos
    /// del gestor. El valor llega en `JobOptions::concurrency`, asi que el ultimo
    /// trabajo creado es el que fija el limite; es la unica lectura coherente de
    /// un ajuste que el usuario ve como una preferencia del equipo y no de cada
    /// trabajo.
    pub fn set_max_concurrent(&self, value: usize) {
        let limit = value.clamp(1, MAX_CONCURRENT);
        self.inner.max_concurrent.store(limit, Ordering::SeqCst);
        self.pump();
    }

    pub fn max_concurrent(&self) -> usize {
        self.inner.max_concurrent.load(Ordering::SeqCst)
    }

    /// Cuantos trabajos esperan turno.
    pub fn queued(&self) -> usize {
        self.inner
            .pending
            .lock()
            .map(|queue| queue.len())
            .unwrap_or(0)
    }

    /// Cuantos trabajos se estan ejecutando ahora mismo.
    pub fn running(&self) -> usize {
        self.inner.running.load(Ordering::SeqCst)
    }

    /// Arranca los trabajos que quepan, por orden de prioridad.
    ///
    /// Se llama al crear un trabajo, al fijar el limite y al terminar uno. El
    /// orden es: mas prioridad primero y, a igualdad, el que llego antes.
    fn pump(&self) {
        loop {
            let limit = self.inner.max_concurrent.load(Ordering::SeqCst);
            let running = self.inner.running.load(Ordering::SeqCst);
            if running >= limit {
                return;
            }

            let Some(next) = self.take_next() else {
                return;
            };

            self.inner.running.fetch_add(1, Ordering::SeqCst);

            let manager = self.clone();
            let job_id = next.id.clone();

            let spawned = std::thread::Builder::new()
                .name(format!("su-job-{job_id}"))
                .spawn(move || {
                    // El turno se devuelve al salir, pase lo que pase. Si el hilo
                    // terminara por un `panic`, un contador desajustado dejaria la
                    // cola parada para siempre.
                    let _slot = SlotGuard { manager: manager.clone() };
                    manager.run(next.id, next.pipeline, next.control, next.context);
                });

            if let Err(error) = spawned {
                // No se pudo arrancar: se devuelve el turno y el trabajo se marca
                // como fallido, en lugar de dejarlo en cola para siempre.
                self.inner.running.fetch_sub(1, Ordering::SeqCst);
                tracing::error!(job = %job_id, error = %error, "no se pudo lanzar el trabajo");
                self.fail_job(&job_id, &format!("no se pudo lanzar el trabajo: {error}"));
                return;
            }
        }
    }

    /// Saca de la cola el siguiente trabajo que toca.
    fn take_next(&self) -> Option<PendingJob> {
        let mut queue = self.inner.pending.lock().ok()?;

        let orden: Vec<(u8, u64)> = queue
            .iter()
            .map(|entry| (entry.priority, entry.sequence))
            .collect();

        let index = next_index(&orden)?;
        queue.remove(index)
    }

    /// Devuelve el turno y arranca lo siguiente. La usa `SlotGuard`.
    fn release_slot(&self) {
        self.inner.running.fetch_sub(1, Ordering::SeqCst);
        self.pump();
    }

    /// Marca un trabajo que no llego a arrancar.
    ///
    /// No se deja en `Queued`: un trabajo que no puede empezar y se queda en cola
    /// es indistinguible de uno que espera turno, y el usuario esperaria para
    /// siempre a que avanzara.
    fn fail_job(&self, job_id: &str, detail: &str) {
        self.set_status(job_id, JobStatus::Failed);

        if let Some(job) = self.get(job_id) {
            let done = job
                .items
                .iter()
                .filter(|item| matches!(item.status, ItemStatus::Done | ItemStatus::Degraded))
                .count();

            self.emit(Event::JobFinished {
                job_id: job_id.to_string(),
                status: JobStatus::Failed,
                done,
                failed: job.items.len().saturating_sub(done),
                degraded: 0,
            });
        }

        tracing::error!(job = %job_id, detail = %detail, "trabajo no arrancado");
        self.persist_job(job_id);
    }

    /// Carga los trabajos guardados en sesiones anteriores.
    ///
    /// Los que quedaron en ejecucion pasan a `Paused`: la interfaz debe ofrecer
    /// reanudarlos, no mostrar un trabajo que dice estar en curso y no avanza.
    /// Devuelve cuantos quedaron interrumpidos.
    pub fn restore(&self) -> SuResult<usize> {
        let Some(store) = self.inner.store.as_ref() else {
            return Ok(0);
        };

        let (all, interrupted) = {
            let store = store
                .lock()
                .map_err(|_| SuError::Internal("el almacen esta bloqueado".to_string()))?;
            // El orden importa: `take_interrupted` pasa a `Paused` los trabajos que
            // quedaron en ejecucion y lo persiste. Si se leyera la lista antes, las
            // copias en memoria conservarian el estado `Running` y la interfaz
            // mostraria un trabajo en curso que no avanza, que es justo lo que se
            // quiere evitar al reabrir la aplicacion.
            let interrupted = store.take_interrupted()?;
            let all = store.list()?;
            (all, interrupted)
        };

        let interrupted_ids: std::collections::HashSet<String> =
            interrupted.into_iter().map(|job| job.id).collect();

        {
            let mut jobs = self
                .inner
                .jobs
                .write()
                .map_err(|_| SuError::Internal("el registro de trabajos esta bloqueado".to_string()))?;

            for job in all {
                jobs.insert(job.id.clone(), job);
            }
        }

        tracing::info!(
            total = interrupted_ids.len(),
            "trabajos restaurados; los interrumpidos quedan en pausa"
        );

        Ok(interrupted_ids.len())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn list(&self) -> Vec<Job> {
        let Ok(jobs) = self.inner.jobs.read() else {
            return Vec::new();
        };
        let mut list: Vec<Job> = jobs.values().cloned().collect();
        // Los mas recientes primero: es el orden en el que el usuario los busca.
        //
        // `created_at` tiene precision de segundo, asi que dos trabajos del mismo
        // lote empatan y el desempate quedaria en manos del orden de recorrido de
        // un `HashMap`, que no es ninguno. El identificador lleva la marca de
        // tiempo en milisegundos y el numero de creacion, asi que ordena donde
        // `created_at` empata.
        list.sort_by(|a, b| (&b.created_at, &b.id).cmp(&(&a.created_at, &a.id)));
        list
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.inner.jobs.read().ok()?.get(id).cloned()
    }

    /// Pausa un trabajo. Se aplica en el siguiente punto seguro.
    ///
    /// Mira y actua en la misma operacion: si el trabajo existe pero su hilo ya
    /// termino, devuelve [`ControlOutcome::NotRunning`] en lugar de un error.
    pub fn pause(&self, id: &str) -> SuResult<ControlOutcome> {
        self.require_job(id)?;
        let Some(control) = self.control_of(id)? else {
            return Ok(ControlOutcome::NotRunning);
        };
        control.pause.store(true, Ordering::Relaxed);
        self.set_status(id, JobStatus::Paused);
        self.emit(Event::JobPaused {
            job_id: id.to_string(),
        });
        Ok(ControlOutcome::Applied)
    }

    /// Quita la pausa a un trabajo **en curso**.
    ///
    /// Reanudar un trabajo que ya no corre no lo relanza: eso es
    /// [`JobManager::resume_or_restart`], que si puede hacerlo. Aqui solo se
    /// despausa, y si no habia hilo se dice con [`ControlOutcome::NotRunning`].
    pub fn resume(&self, id: &str) -> SuResult<ControlOutcome> {
        self.require_job(id)?;
        let Some(control) = self.control_of(id)? else {
            return Ok(ControlOutcome::NotRunning);
        };
        control.pause.store(false, Ordering::Relaxed);
        self.set_status(id, JobStatus::Running);
        self.emit(Event::JobResumed {
            job_id: id.to_string(),
        });
        Ok(ControlOutcome::Applied)
    }

    pub fn cancel(&self, id: &str) -> SuResult<ControlOutcome> {
        self.require_job(id)?;
        let Some(control) = self.control_of(id)? else {
            return Ok(ControlOutcome::NotRunning);
        };
        control.cancel.store(true, Ordering::Relaxed);
        control.pause.store(false, Ordering::Relaxed);
        Ok(ControlOutcome::Applied)
    }

    /// `true` si el trabajo tiene un hilo de ejecucion activo.
    pub fn is_alive(&self, id: &str) -> bool {
        self.inner
            .control
            .read()
            .map(|controls| controls.contains_key(id))
            .unwrap_or(false)
    }

    /// Reanuda un trabajo, haga falta despausarlo o relanzarlo.
    ///
    /// Es lo que espera un cliente al pulsar "reanudar": no deberia tener que
    /// saber si el trabajo sigue vivo en memoria o viene de una sesion anterior.
    ///
    /// La decision la toma el gestor y no el llamador: preguntar "sigue vivo"
    /// desde fuera y actuar despues dejaba una ventana en la que el trabajo
    /// terminaba entre las dos llamadas, y quien pedia reanudar recibia un error
    /// por algo que habia ido bien.
    pub fn resume_or_restart(&self, id: &str, context: JobContext) -> SuResult<Job> {
        match self.resume(id)? {
            ControlOutcome::Applied => self
                .get(id)
                .ok_or_else(|| SuError::Internal(format!("no existe el trabajo '{id}'"))),
            ControlOutcome::NotRunning => self.resume_job(id, context),
        }
    }

    /// Reanuda un trabajo que no esta corriendo.
    ///
    /// Cubre los dos casos reales: un trabajo restaurado tras un cierre
    /// inesperado y uno cancelado por el usuario.
    ///
    /// No hace falta tocar el estado de los items: el bucle de ejecucion ya se
    /// salta los que estan resueltos y cuenta los que ya lo estaban. Esa es la
    /// ventaja de tener una unica ruta de ejecucion para el arranque y la
    /// reanudacion, en lugar de dos parecidas que se desincronizan.
    pub fn resume_job(&self, id: &str, context: JobContext) -> SuResult<Job> {
        let job = self
            .get(id)
            .ok_or_else(|| SuError::Internal(format!("no existe el trabajo '{id}'")))?;

        if job.status == JobStatus::Running {
            return Err(SuError::Internal(format!(
                "el trabajo '{id}' ya esta en ejecucion"
            )));
        }

        let pending = job
            .items
            .iter()
            .filter(|item| matches!(item.status, ItemStatus::Pending | ItemStatus::Failed))
            .count();

        if pending == 0 {
            return Err(SuError::NoImagesFound);
        }

        let pipeline = context
            .pipelines
            .get(job.mode, job.scale)
            .ok_or_else(|| {
                SuError::Internal(format!(
                    "no hay pipeline para modo {} y escala x{}",
                    job.mode.as_str(),
                    job.scale.factor()
                ))
            })?
            .clone();

        self.set_status(id, JobStatus::Queued);

        let control = JobControl::default();
        {
            let mut controls = self.inner.control.write().map_err(|_| {
                SuError::Internal("el registro de trabajos esta bloqueado".to_string())
            })?;
            controls.insert(id.to_string(), control.clone());
        }

        tracing::info!(job = %id, pending, "reanudando trabajo");

        // La reanudacion pasa por la misma cola que la creacion: un trabajo
        // reanudado compite por el turno igual que los demas, en lugar de
        // arrancar un hilo suelto que se saltaria el limite de concurrencia.
        // `Job` no guarda la prioridad de la peticion original, asi que un trabajo
        // reanudado entra con la prioridad por defecto y con el numero de creacion
        // mas bajo: entre iguales, el que ya existia va primero.
        match self.inner.pending.lock() {
            Ok(mut queue) => queue.push_back(PendingJob {
                id: id.to_string(),
                priority: 0,
                sequence: 0,
                pipeline,
                control,
                context,
            }),
            Err(_) => {
                return Err(SuError::Internal(
                    "la cola de trabajos esta bloqueada".to_string(),
                ))
            }
        }

        self.pump();

        self.get(id)
            .ok_or_else(|| SuError::Internal("el trabajo desaparecio al reanudarlo".to_string()))
    }

    /// Devuelve el control de un trabajo, o `None` si su hilo ya termino.
    ///
    /// No es un error que no haya control: significa que el trabajo no esta en
    /// curso. Quien llama decide si eso le importa (`pause`, `cancel`) o si debe
    /// relanzarlo (`resume_or_restart`).
    fn control_of(&self, id: &str) -> SuResult<Option<JobControl>> {
        let controls = self
            .inner
            .control
            .read()
            .map_err(|_| SuError::Internal("el registro de trabajos esta bloqueado".to_string()))?;

        Ok(controls.get(id).cloned())
    }

    /// Comprueba que el trabajo existe.
    ///
    /// Un id desconocido si es un error del llamador, y se dice con un mensaje
    /// que no se puede confundir con "el trabajo ya termino".
    fn require_job(&self, id: &str) -> SuResult<()> {
        if self.get(id).is_none() {
            return Err(SuError::Internal(format!("no existe el trabajo '{id}'")));
        }
        Ok(())
    }

    fn emit(&self, event: Event) {
        // Si no hay nadie suscrito, `send` devuelve error; no es un problema.
        let _ = self.inner.events.send(event);
    }

    fn set_status(&self, id: &str, status: JobStatus) {
        if let Ok(mut jobs) = self.inner.jobs.write() {
            if let Some(job) = jobs.get_mut(id) {
                job.status = status;
            }
        }
    }

    /// Identificador unico y ordenable.
    ///
    /// Lleva marca de tiempo ademas de secuencia para que sea unico **entre
    /// reinicios** sin tener que restaurar un contador desde el disco. Eso
    /// elimina toda una clase de fallo: reanudar sesion y generar un id que ya
    /// existe, sobrescribiendo un trabajo anterior.
    ///
    /// La secuencia es **global al proceso**, no de cada gestor: con un contador
    /// por gestor, dos gestores creados en el mismo milisegundo (una prueba, o un
    /// reinicio de la cola al cambiar de configuracion) producian el mismo
    /// identificador. Ver `next_sequence`.
    ///
    /// Solo la usan los tests de unicidad: crear un trabajo pasa por
    /// `next_id_and_sequence`, que devuelve tambien el numero de creacion y evita
    /// que las dos cosas se puedan cruzar.
    #[cfg(test)]
    fn next_id(&self) -> String {
        self.next_id_and_sequence().0
    }

    /// El identificador y el numero de creacion, que desempata las prioridades.
    ///
    /// Se devuelven juntos y no en dos llamadas para que no se puedan cruzar: dos
    /// hilos creando trabajos a la vez podrian leer numeros distintos y romper el
    /// orden de llegada.
    fn next_id_and_sequence(&self) -> (String, u64) {
        let sequence = next_sequence();
        (
            format!("job-{}-{:04}", now_unix_ms(), sequence),
            sequence,
        )
    }

    /// Guarda el estado actual del trabajo, si hay almacen.
    ///
    /// Un fallo al persistir se registra pero **no detiene el lote**: perder el
    /// registro es malo, pero matar un trabajo que el usuario esta esperando
    /// porque el disco se lleno es peor.
    fn persist_job(&self, job_id: &str) {
        let Some(store) = self.inner.store.as_ref() else {
            return;
        };

        let Ok(jobs) = self.inner.jobs.read() else {
            return;
        };
        let Some(job) = jobs.get(job_id) else {
            return;
        };
        let Ok(store) = store.lock() else {
            return;
        };

        if let Err(error) = store.save(job) {
            tracing::warn!(%error, job = %job_id, "no se pudo persistir el trabajo");
        }
    }

    /// Crea un trabajo y lanza su ejecucion en un hilo propio.
    ///
    /// Devuelve el trabajo en estado `Queued`: el llamador puede responder al
    /// cliente inmediatamente y seguir el avance por el canal de eventos.
    pub fn create(&self, request: JobRequest, context: JobContext) -> SuResult<Job> {
        if request.items.is_empty() {
            return Err(SuError::NoImagesFound);
        }

        let pipeline = context
            .pipelines
            .get(request.mode, request.scale)
            .ok_or_else(|| {
                SuError::Internal(format!(
                    "no hay pipeline para modo {} y escala x{}",
                    request.mode.as_str(),
                    request.scale.factor()
                ))
            })?
            .clone();

        let (id, sequence) = self.next_id_and_sequence();
        let created_at = now_iso8601();

        let items: Vec<JobItem> = request
            .items
            .iter()
            .enumerate()
            .map(|(index, path)| JobItem {
                id: format!("{id}-{index:04}"),
                src_path: path.clone(),
                out_path: None,
                status: ItemStatus::Pending,
                attempt: 0,
                progress: 0.0,
                stage: None,
                duration_ms: None,
                vram_peak_mb: None,
                effective_pipeline: Vec::new(),
                skipped: Vec::new(),
                error_code: None,
                error_detail: None,
                degraded: false,
            })
            .collect();

        let job = Job {
            id: id.clone(),
            created_at,
            status: JobStatus::Queued,
            mode: request.mode,
            scale: request.scale,
            pipeline_id: pipeline.id.clone(),
            output: request.output.clone(),
            options: request.options.clone(),
            progress: JobProgress {
                total: items.len(),
                done: 0,
                failed: 0,
                degraded: 0,
            },
            items,
        };

        let control = JobControl::default();

        {
            let mut jobs = self
                .inner
                .jobs
                .write()
                .map_err(|_| SuError::Internal("el registro de trabajos esta bloqueado".to_string()))?;
            jobs.insert(id.clone(), job.clone());
        }
        {
            let mut controls = self
                .inner
                .control
                .write()
                .map_err(|_| SuError::Internal("el registro de trabajos esta bloqueado".to_string()))?;
            controls.insert(id.clone(), control.clone());
        }

        self.emit(Event::JobCreated {
            job_id: id.clone(),
            total: job.items.len(),
        });
        self.persist_job(&id);

        // El limite de concurrencia es global y viaja en las opciones del trabajo.
        // Se aplica antes de encolar para que este mismo trabajo ya lo respete.
        self.set_max_concurrent(usize::from(request.options.concurrency));

        // Se encola en lugar de arrancarlo: quien decide si hay turno es `pump`,
        // que respeta el limite y el orden de prioridad.
        match self.inner.pending.lock() {
            Ok(mut queue) => queue.push_back(PendingJob {
                id: id.clone(),
                priority: request.priority,
                sequence,
                pipeline,
                control,
                context,
            }),
            Err(_) => {
                return Err(SuError::Internal(
                    "la cola de trabajos esta bloqueada".to_string(),
                ))
            }
        }

        // Arranca ya si hay turno. Si no, se queda esperando y `pump` lo lanzara
        // cuando termine el trabajo que ocupa el sitio.
        self.pump();

        Ok(job)
    }

    /// Bucle de ejecucion de un trabajo. Se ejecuta en su propio hilo.
    fn run(
        &self,
        job_id: String,
        pipeline: Pipeline,
        control: JobControl,
        context: JobContext,
    ) {
        let Some(job) = self.get(&job_id) else {
            return;
        };

        self.set_status(&job_id, JobStatus::Running);
        self.emit(Event::JobStarted {
            job_id: job_id.clone(),
            total: job.items.len(),
        });

        // Los contadores se siembran con lo que ya estaba resuelto en una sesion
        // anterior. `failed` arranca en cero a proposito: los items fallidos se
        // reintentan, asi que contarlos ahora los contaria dos veces. El
        // resultado final refleja lo que ha pasado en esta ejecucion.
        let mut done = job
            .items
            .iter()
            .filter(|item| matches!(item.status, ItemStatus::Done | ItemStatus::Degraded))
            .count();
        let mut failed = 0usize;
        let mut degraded = job
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::Degraded)
            .count();
        let mut cancelled = false;

        for item in job.items.clone() {
            // Un item ya resuelto no se repite. Es lo que hace correcta la
            // reanudacion: las imagenes terminadas en la sesion anterior se
            // quedan como estan y solo se procesan las pendientes y las fallidas.
            if matches!(
                item.status,
                ItemStatus::Done | ItemStatus::Degraded | ItemStatus::Skipped
            ) {
                continue;
            }

            // Punto seguro: la pausa y la cancelacion se resuelven entre imagenes,
            // nunca a mitad de una inferencia.
            while control.is_paused() && !control.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            if control.is_cancelled() {
                cancelled = true;
                self.mark_remaining(&job_id, &item.id);
                break;
            }

            self.emit(Event::ItemStarted {
                job_id: job_id.clone(),
                item_id: item.id.clone(),
                name: file_name(&item.src_path),
            });

            let started = std::time::Instant::now();

            match self.process_item(
                &job_id,
                &item,
                &pipeline,
                &job.output,
                &job.options,
                &context,
                &control,
            ) {
                Ok(outcome) => {
                    let duration_ms = started.elapsed().as_millis() as u64;
                    if outcome.degraded {
                        degraded += 1;
                    }
                    done += 1;

                    self.update_item(&job_id, &item.id, |entry| {
                        entry.status = if outcome.degraded {
                            ItemStatus::Degraded
                        } else {
                            ItemStatus::Done
                        };
                        entry.out_path = Some(outcome.out_path.clone());
                        entry.progress = 1.0;
                        entry.stage = None;
                        entry.duration_ms = Some(duration_ms);
                        entry.effective_pipeline = outcome.executed.clone();
                        // El motivo tiene que sobrevivir al trabajo. Se emitia en
                        // `ItemCompleted`, pero eso solo lo ve un cliente que este
                        // conectado justo en ese momento: al recargar, el item
                        // volvia sin explicacion.
                        entry.skipped = outcome.skipped.clone();
                        entry.degraded = outcome.degraded;
                    });

                    self.emit(Event::ItemCompleted {
                        job_id: job_id.clone(),
                        item_id: item.id.clone(),
                        out_path: outcome.out_path,
                        duration_ms,
                        executed: outcome.executed,
                        skipped: outcome.skipped,
                        degraded: outcome.degraded,
                    });
                }

                Err(error) => {
                    let duration_ms = started.elapsed().as_millis() as u64;
                    let code = error.code().as_str().to_string();
                    let detail = error.to_string();

                    failed += 1;
                    self.update_item(&job_id, &item.id, |entry| {
                        entry.status = ItemStatus::Failed;
                        entry.progress = 0.0;
                        entry.stage = None;
                        entry.duration_ms = Some(duration_ms);
                        entry.error_code = Some(code.clone());
                        entry.error_detail = Some(detail.clone());
                    });

                    tracing::warn!(job = %job_id, item = %item.id, code = %code, "item fallido");

                    self.emit(Event::ItemFailed {
                        job_id: job_id.clone(),
                        item_id: item.id.clone(),
                        code,
                        detail,
                    });
                }
            }

            self.set_progress(&job_id, done, failed, degraded, job.items.len());
            self.emit(Event::JobProgress {
                job_id: job_id.clone(),
                done,
                failed,
                degraded,
                total: job.items.len(),
            });
            self.persist_job(&job_id);

            // `unloadBetweenImages` cambia pico de memoria por tiempo de recarga:
            // es lo que promete el ajuste, y por eso se suelta aqui, en la frontera
            // entre imagenes, y no al final del lote (que no ahorraria nada).
            if job.options.unload_between_images {
                context.provider.unload();
            }
        }

        let status = if cancelled {
            JobStatus::Cancelled
        } else if failed == 0 {
            JobStatus::Completed
        } else if done == 0 {
            JobStatus::Failed
        } else {
            JobStatus::Partial
        };

        self.set_status(&job_id, status);
        self.emit(Event::JobFinished {
            job_id: job_id.clone(),
            status,
            done,
            failed,
            degraded,
        });
        self.persist_job(&job_id);

        if let Ok(mut controls) = self.inner.control.write() {
            controls.remove(&job_id);
        }

        tracing::info!(job = %job_id, ?status, done, failed, degraded, "trabajo terminado");
    }

    // Ocho parametros a proposito: el item, el pipeline y el contexto del trabajo
    // son cosas distintas, y agruparlas en una estructura solo para bajar la cuenta
    // las esconderria detras de un nombre. Es la misma razon por la que
    // `upscale_tiled` la lleva.
    #[allow(clippy::too_many_arguments)]
    fn process_item(
        &self,
        job_id: &str,
        item: &JobItem,
        pipeline: &Pipeline,
        output: &OutputSettings,
        options: &JobOptions,
        context: &JobContext,
        control: &JobControl,
    ) -> SuResult<ItemOutcome> {
        let source = su_imageio::decode(std::path::Path::new(&item.src_path))?;

        let analysis: Analysis = analyze(&source, pipeline.scale);
        let vars = build_vars(&analysis, &pipeline.mode, pipeline.scale, options, context);
        let runner_config = build_runner_config(&analysis, options, context);

        let job_id_owned = job_id.to_string();
        let item_id_owned = item.id.clone();
        let events = self.inner.events.clone();

        let mut last_emit = std::time::Instant::now();
        let mut last_percent = 0.0f32;

        let mut on_progress = |progress: su_inference::StageProgress| {
            let percent = if progress.total == 0 {
                0.0
            } else {
                progress.done as f32 / progress.total as f32
            };

            // Coalescencia: como maximo un evento cada 100 ms por item, o el
            // canal se satura con imagenes troceadas en cientos de tiles.
            let now = std::time::Instant::now();
            let should_emit = now.duration_since(last_emit).as_millis() >= 100
                || (percent - last_percent).abs() >= 0.05
                || progress.done == progress.total;

            if should_emit {
                last_emit = now;
                last_percent = percent;
                let _ = events.send(Event::ItemProgress {
                    job_id: job_id_owned.clone(),
                    item_id: item_id_owned.clone(),
                    stage: progress.stage,
                    done: progress.done,
                    total: progress.total,
                    percent,
                });
            }
        };

        // El color de un pixel totalmente transparente es indefinido (depende de
        // como se genero el archivo) y el modelo lo tomaria como si fuera parte de
        // la imagen, inventando un contorno entre ese color de fondo y el dibujo.
        // Se rellena con el del pixel visible mas cercano antes de inferir; el alfa
        // que se recompone al final sigue siendo el del original.
        let prepared = su_imageio::bleed_transparent(&source);

        let (outcome, fallback_reason) = run_with_fallback(
            &prepared,
            pipeline,
            &vars,
            context,
            &runner_config,
            &mut on_progress,
        )?;

        if control.is_cancelled() {
            return Err(SuError::Cancelled);
        }

        // El canal alfa ya viene escalado por el runner, con el mismo modelo y la
        // misma malla de tiles que el color. Antes se interpolaba aqui con Lanczos,
        // y eso dejaba la silueta en una rampa de 4 a 7 px mientras el color salia
        // con el borde reconstruido en 1 px: el halo blando del contorno de toda la
        // vida.
        let image = outcome.image;

        let target = su_imageio::output_path(
            std::path::Path::new(&item.src_path),
            std::path::Path::new(&output.dir),
            &output.suffix,
            output.format,
        );

        su_imageio::write_atomic(&image, &target, output.format, output.quality)?;

        let mut skipped: Vec<String> = outcome
            .skipped
            .into_iter()
            .map(|(stage, reason)| format!("{}: {reason}", stage.as_str()))
            .collect();

        // Que la imagen se hiciera con otro motor hay que decirlo en el mismo sitio
        // en el que se dice todo lo demas que la aparta del pipeline declarado: el
        // resumen del lote y las notas del item. Un resultado interpolado que se
        // presenta como si lo hubiera hecho el modelo es la peor forma de fallar.
        let interpolated = fallback_reason.is_some();
        if let Some(reason) = fallback_reason {
            skipped.push(reason);
        }

        Ok(ItemOutcome {
            out_path: target.display().to_string(),
            executed: outcome.executed,
            skipped,
            degraded: outcome.degradations > 0 || interpolated,
        })
    }

    fn update_item(&self, job_id: &str, item_id: &str, update: impl FnOnce(&mut JobItem)) {
        let Ok(mut jobs) = self.inner.jobs.write() else {
            return;
        };
        let Some(job) = jobs.get_mut(job_id) else {
            return;
        };
        let Some(item) = job.items.iter_mut().find(|entry| entry.id == item_id) else {
            return;
        };
        update(item);
    }

    fn set_progress(&self, job_id: &str, done: usize, failed: usize, degraded: usize, total: usize) {
        let Ok(mut jobs) = self.inner.jobs.write() else {
            return;
        };
        let Some(job) = jobs.get_mut(job_id) else {
            return;
        };
        job.progress = JobProgress {
            total,
            done,
            failed,
            degraded,
        };
    }

    /// Marca como omitidos los items que no llegan a ejecutarse por cancelacion.
    fn mark_remaining(&self, job_id: &str, from_item_id: &str) {
        let Ok(mut jobs) = self.inner.jobs.write() else {
            return;
        };
        let Some(job) = jobs.get_mut(job_id) else {
            return;
        };
        let mut reached = false;
        for item in job.items.iter_mut() {
            if item.id == from_item_id {
                reached = true;
            }
            if reached && item.status == ItemStatus::Pending {
                item.status = ItemStatus::Skipped;
            }
        }
    }
}

struct ItemOutcome {
    out_path: String,
    executed: Vec<StageId>,
    skipped: Vec<String>,
    degraded: bool,
}

/// Directorio de salida de un item.
/// Ejecuta el pipeline y, si el motor no puede trabajar por un modelo, lo
/// reintenta con el de respaldo.
///
/// Devuelve el resultado y, si hubo que recurrir al respaldo, el motivo para el
/// informe: es una cadena porque acaba tal cual en las notas del item, que es
/// donde el usuario las lee.
///
/// El reintento es **un solo intento**, no una escalera: el respaldo no depende
/// de la imagen (no hay ninguna razon para que la segunda vuelta funcione si la
/// primera no), y una escalera convertiria un fallo claro en dos minutos de
/// espera antes del mismo error.
fn run_with_fallback(
    source: &su_imageio::DecodedImage,
    pipeline: &Pipeline,
    vars: &EvalVars,
    context: &JobContext,
    config: &RunnerConfig,
    on_progress: &mut dyn FnMut(su_inference::StageProgress),
) -> SuResult<(RunOutcome, Option<String>)> {
    match run_pipeline(
        source,
        pipeline,
        vars,
        context.provider.as_ref(),
        config,
        &mut *on_progress,
    ) {
        Ok(outcome) => Ok((outcome, None)),

        Err(error) if is_model_unavailable(&error) => {
            let Some(fallback) = context.fallback.as_ref() else {
                return Err(error);
            };

            tracing::warn!(
                error = %error,
                motor = %context.provider_name,
                respaldo = fallback.name(),
                "el motor elegido no puede con este pipeline: se reintenta con el de respaldo"
            );

            match run_pipeline(
                source,
                pipeline,
                vars,
                fallback.as_ref(),
                config,
                &mut *on_progress,
            ) {
                Ok(outcome) => Ok((
                    outcome,
                    Some(format!(
                        "motor de respaldo '{}': el elegido no pudo usarse ({error})",
                        fallback.name()
                    )),
                )),

                // Si el respaldo tampoco puede, se devuelve **el error original**:
                // es el que dice que modelo falta, y eso si puede arreglarlo el
                // usuario. El del respaldo seria un aviso sobre interpolacion que no
                // le dice nada util.
                Err(_) => Err(error),
            }
        }

        Err(other) => Err(other),
    }
}

/// `true` si el fallo significa "este motor no puede con este pipeline".
///
/// Son los cuatro casos en los que no hay nada que reintentar contra el mismo
/// motor: falta el modelo, el archivo no es el que declara el manifiesto, el
/// execution provider no arranca o TensorRT no construye el motor. Los fallos de
/// memoria **no** estan aqui: esos los resuelve el runner bajando el tile, y
/// mandarlos al respaldo perderia la calidad por un problema pasajero.
fn is_model_unavailable(error: &SuError) -> bool {
    matches!(
        error,
        SuError::ModelMissing(_)
            | SuError::ModelHashMismatch { .. }
            | SuError::ExecutionProviderUnavailable { .. }
            | SuError::TensorRtEngineBuildFailed(_)
    )
}

/// Construye las variables que evaluan las condiciones del pipeline.
///
/// Las preferencias del usuario salen de `options` y no de valores fijos: las
/// condiciones del pipeline son las que deciden si una etapa entra, asi que fijar
/// aqui `sharpen: false` o `denoise: Auto` convierte los ajustes avanzados de la
/// interfaz en botones que no hacen nada. La interfaz ya dibujaba un plan que los
/// tenia en cuenta, asi que ademas mentia sobre lo que se iba a ejecutar.
pub fn build_vars(
    analysis: &Analysis,
    mode: &Mode,
    scale: Scale,
    options: &JobOptions,
    context: &JobContext,
) -> EvalVars {
    EvalVars {
        noise: analysis.noise,
        blockiness: analysis.blockiness,
        face_count: analysis.reliable_face_count(),
        // Las cajas, no solo el numero: la etapa `face` recorta cada rostro para
        // pasarlo por el modelo, y para eso necesita saber donde esta. Se filtran
        // aqui una vez, con el mismo criterio (`is_reliable`) que cuenta
        // `face_count`, para que el numero y las cajas no puedan discrepar.
        faces: analysis
            .faces
            .iter()
            .copied()
            .filter(su_core::FaceBox::is_reliable)
            .collect(),
        kind: analysis.kind,
        kind_confidence: analysis.kind_confidence,
        megapixels: (analysis.width as f32 * analysis.height as f32) / 1_000_000.0,
        estimated_output_mp: analysis.estimated_output_mp,
        has_alpha: analysis.has_alpha,
        mode: *mode,
        scale,
        face_restore: options.face_restore,
        denoise: options.denoise,
        sharpen: options.sharpen,
        model_chain_mode: options.model_chain_mode,
        max_output_mp: 800.0,
        free_vram_mb: context.free_vram_mb,
        is_cpu: context.is_cpu,
        provider: context.provider_name.clone(),
        cores: context.cores,
    }
}

/// Ajusta la configuracion del runner a la imagen concreta.
///
/// El tile declarado por el modelo es el punto de partida; si la imagen es
/// pequena, trocearla solo anadiria costuras y trabajo.
fn build_runner_config(
    analysis: &Analysis,
    options: &JobOptions,
    context: &JobContext,
) -> RunnerConfig {
    let mut config = context.runner.clone();

    // El tile pedido por el usuario manda. `context.runner` trae el del equipo; si
    // el trabajo dice otro, gana el del trabajo, porque es el que el usuario acaba
    // de elegir en la pantalla.
    config.tile_choice = options.tile_size;

    // En modo Manual, el modelo elegido sustituye al de las etapas de escalado.
    // En Automatico la cadena la decide el analisis, asi que no se toca.
    config.model_override = match options.model_chain_mode {
        su_core::ModelChainMode::Manual => options.upscale_model.clone(),
        su_core::ModelChainMode::Auto => None,
    };

    config.candidates = config
        .candidates
        .iter()
        .copied()
        .filter(|candidate| *candidate >= su_tiling::MIN_TILE)
        .collect();

    if config.candidates.is_empty() {
        config.candidates = su_tiling::DEFAULT_CANDIDATES.to_vec();
    }

    // Si la imagen cabe entera con margen, no hay razon para trocear. Solo se
    // aplica cuando el usuario no ha pedido un tile concreto: forzar `Auto` sobre
    // una eleccion explicita haria que el ajuste no sirviera para nada.
    let largest = analysis.width.max(analysis.height);
    if largest <= 512 && config.tile_choice.explicit().is_none() {
        config.tile_choice = su_core::TileChoice::Auto;
    }

    config
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// Numero de creacion, unico y creciente dentro del proceso.
///
/// Es global y no de cada gestor a proposito. Con el contador dentro del gestor,
/// dos gestores creados en el mismo milisegundo generaban el mismo identificador
/// (`job-<ms>-0001`): el caso no es teorico, es el de cualquier reconfiguracion
/// que recree la cola sin reiniciar el proceso, y el resultado es que un trabajo
/// nuevo sobrescribe el registro de otro en la base de datos.
///
/// `fetch_add` resuelve ademas la carrera entre hilos que crean trabajos a la vez,
/// que es para lo que se tomaba el candado del contador anterior.
fn next_sequence() -> u64 {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1
}

/// Marca de tiempo UNIX en milisegundos.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Marca de tiempo ISO 8601 en UTC.
pub fn now_iso8601() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Opciones por defecto para un trabajo creado desde la API sin `options`.
pub fn default_options() -> JobOptions {
    JobOptions::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use su_core::{
        DenoiseChoice, FaceRestoreChoice, ModelChainMode, OutputFormat, OutputSettings, TileChoice,
    };
    use su_inference::MockBackendProvider;

    /// Analisis sintetico. El ruido va alto a proposito: es lo que activa las etapas
    /// condicionales, que son justo las que dependen de las preferencias del usuario.
    fn analysis_of(width: u32, height: u32, scale: Scale) -> Analysis {
        Analysis {
            width,
            height,
            has_alpha: false,
            noise: 0.5,
            blockiness: 0.1,
            kind: su_core::ContentKind::Unknown,
            kind_confidence: 0.0,
            faces: Vec::new(),
            exif_orientation: 1,
            estimated_output_mp: (width as f32 * height as f32 / 1_000_000.0)
                * (scale.factor() * scale.factor()) as f32,
        }
    }

    #[test]
    fn the_users_advanced_settings_reach_the_pipeline_conditions() {
        // Las condiciones del pipeline son las que deciden si una etapa entra, asi
        // que fijarlas a mano aqui convierte los ajustes avanzados de la interfaz en
        // botones que no hacen nada. Ademas la vista previa del plan los tenia en
        // cuenta, con lo que mentia sobre lo que se iba a ejecutar.
        let analysis = analysis_of(64, 64, Scale::X4);
        let ctx = context(std::env::temp_dir());

        let options = JobOptions {
            sharpen: true,
            denoise: DenoiseChoice::Off,
            face_restore: FaceRestoreChoice::High,
            model_chain_mode: ModelChainMode::Manual,
            ..JobOptions::default()
        };

        let vars = build_vars(&analysis, &Mode::Photo, Scale::X4, &options, &ctx);

        assert!(vars.sharpen, "el enfoque pedido tiene que llegar");
        assert_eq!(vars.denoise, DenoiseChoice::Off, "un 'off' explicito no puede activar el denoise");
        assert_eq!(vars.face_restore, FaceRestoreChoice::High);
        assert_eq!(vars.model_chain_mode, ModelChainMode::Manual);
    }

    #[test]
    fn the_face_boxes_reach_the_pipeline_and_agree_with_the_count() {
        // La etapa facial recorta cada rostro, asi que necesita las cajas, no solo
        // cuantos hay. Y las dos cosas tienen que salir del mismo filtro: una caja
        // poco fiable que se contara dejaria a la etapa buscando una cara que no
        // esta en la lista, y una caja fiable no contada dejaria la etapa apagada.
        let mut analysis = analysis_of(256, 256, Scale::X4);
        analysis.faces = vec![
            su_core::FaceBox {
                x: 0.30,
                y: 0.30,
                w: 0.20,
                h: 0.20,
                confidence: 0.9,
            },
            su_core::FaceBox {
                x: 0.70,
                y: 0.70,
                w: 0.02,
                h: 0.02,
                confidence: 0.2,
            },
        ];

        let ctx = context(std::env::temp_dir());
        let vars = build_vars(
            &analysis,
            &Mode::Photo,
            Scale::X4,
            &JobOptions::default(),
            &ctx,
        );

        assert_eq!(vars.face_count, 1, "solo una caja es fiable");
        assert_eq!(vars.faces.len(), vars.face_count);
        assert_eq!(vars.faces[0].confidence, 0.9);
    }

    #[test]
    fn a_manual_model_and_an_explicit_tile_reach_the_runner() {
        let analysis = analysis_of(2048, 2048, Scale::X4);
        let ctx = context(std::env::temp_dir());

        let options = JobOptions {
            tile_size: TileChoice::Px256,
            model_chain_mode: ModelChainMode::Manual,
            upscale_model: Some("realesrgan-x4plus".to_string()),
            ..JobOptions::default()
        };

        let config = build_runner_config(&analysis, &options, &ctx);

        assert_eq!(config.tile_choice, TileChoice::Px256);
        assert_eq!(config.model_override.as_deref(), Some("realesrgan-x4plus"));
    }

    #[test]
    fn the_automatic_mode_ignores_a_manual_model() {
        // El modelo manual solo manda en modo Manual. En Automatico la cadena la
        // decide el analisis, y `upscaleModel` puede venir de una sesion anterior.
        let analysis = analysis_of(512, 512, Scale::X4);
        let ctx = context(std::env::temp_dir());

        let options = JobOptions {
            model_chain_mode: ModelChainMode::Auto,
            upscale_model: Some("realesrgan-x4plus".to_string()),
            ..JobOptions::default()
        };

        assert!(build_runner_config(&analysis, &options, &ctx)
            .model_override
            .is_none());
    }

    #[test]
    fn a_small_image_does_not_override_an_explicit_tile() {
        // "La imagen cabe entera, no hace falta trocear" es una optimizacion, y no
        // puede pisar una eleccion explicita: el ajuste existe justo para forzar el
        // troceado cuando se quiere reproducir un fallo de costuras.
        let analysis = analysis_of(64, 64, Scale::X4);
        let ctx = context(std::env::temp_dir());

        let explicit = JobOptions {
            tile_size: TileChoice::Px256,
            ..JobOptions::default()
        };
        assert_eq!(
            build_runner_config(&analysis, &explicit, &ctx).tile_choice,
            TileChoice::Px256
        );

        let automatic = JobOptions::default();
        assert_eq!(
            build_runner_config(&analysis, &automatic, &ctx).tile_choice,
            TileChoice::Auto
        );
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("su-jobs-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("directorio temporal");
        dir
    }

    /// Imagen con degradado, guardada como PNG real.
    ///
    /// Se construye un `DecodedImage` y se escribe con `su-imageio` en lugar de
    /// usar la crate `image` directamente: `su-jobs` no depende de ella, y
    /// anadirla solo para los tests ampliaria el arbol de dependencias sin motivo.
    fn write_source(dir: &std::path::Path, name: &str, width: u32, height: u32) -> PathBuf {
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.push((x % 256) as f32 / 255.0);
                rgb.push((y % 256) as f32 / 255.0);
                rgb.push(((x + y) % 256) as f32 / 255.0);
            }
        }

        let image = su_imageio::DecodedImage {
            width,
            height,
            rgb,
            alpha: None,
            icc_profile: None,
            applied_orientation: 1,
        };

        let path = dir.join(name);
        su_imageio::write_atomic(&image, &path, OutputFormat::Png, 95)
            .expect("no se pudo escribir la imagen de prueba");
        path
    }

    fn context(models_dir: PathBuf) -> JobContext {
        JobContext {
            models_dir,
            registry: Arc::new(
                ModelRegistry::from_json(r#"{"manifestVersion":2,"models":[]}"#)
                    .expect("manifiesto vacio"),
            ),
            provider: Arc::new(MockBackendProvider),
            fallback: None,
            pipelines: Arc::new(PipelineSet::embedded().expect("pipelines")),
            runner: RunnerConfig::default(),
            provider_name: "CPU".to_string(),
            is_cpu: true,
            free_vram_mb: 0,
            cores: 4,
        }
    }

    fn request(items: Vec<String>, output_dir: &std::path::Path) -> JobRequest {
        JobRequest {
            mode: Mode::Photo,
            scale: Scale::X4,
            items,
            output: OutputSettings {
                dir: output_dir.display().to_string(),
                format: OutputFormat::Png,
                quality: 95,
                suffix: String::new(),
                preserve_metadata: true,
                zip_output: false,
            },
            options: JobOptions::default(),
            priority: 0,
        }
    }

    fn wait_for_finish(manager: &JobManager, job_id: &str, timeout_ms: u64) -> Job {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if let Some(job) = manager.get(job_id) {
                if matches!(
                    job.status,
                    JobStatus::Completed
                        | JobStatus::Partial
                        | JobStatus::Failed
                        | JobStatus::Cancelled
                ) {
                    return job;
                }
            }
            if std::time::Instant::now() > deadline {
                panic!("el trabajo no termino en {timeout_ms} ms");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn an_empty_request_is_rejected() {
        let dir = scratch("empty");
        let manager = JobManager::new(64);
        let error = manager
            .create(request(Vec::new(), &dir), context(dir.clone()))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E001");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_runs_to_completion_and_reports_every_item() {
        let dir = scratch("run");
        let a = write_source(&dir, "a.png", 64, 48);
        let b = write_source(&dir, "b.png", 32, 32);

        let manager = JobManager::new(256);
        let mut events = manager.subscribe();

        let job = manager
            .create(
                request(
                    vec![a.display().to_string(), b.display().to_string()],
                    &dir,
                ),
                context(dir.clone()),
            )
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);

        assert_eq!(finished.status, JobStatus::Completed);
        assert_eq!(finished.progress.done, 2);
        assert_eq!(finished.progress.failed, 0);
        assert!(finished
            .items
            .iter()
            .all(|item| item.status == ItemStatus::Done));
        assert!(finished.items.iter().all(|item| item.duration_ms.is_some()));

        // Debe haberse emitido al menos un evento de cada tipo relevante.
        let mut saw_started = false;
        let mut saw_completed = false;
        while let Ok(event) = events.try_recv() {
            match event {
                Event::JobStarted { .. } => saw_started = true,
                Event::ItemCompleted { .. } => saw_completed = true,
                _ => {}
            }
        }
        assert!(saw_started, "no se emitio JobStarted");
        assert!(saw_completed, "no se emitio ItemCompleted");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_fails_that_item_but_not_the_batch() {
        let dir = scratch("partial");
        let good = write_source(&dir, "buena.png", 32, 32);

        let manager = JobManager::new(256);
        let job = manager
            .create(
                request(
                    vec![
                        good.display().to_string(),
                        dir.join("no-existe.png").display().to_string(),
                    ],
                    &dir,
                ),
                context(dir.clone()),
            )
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);

        assert_eq!(finished.status, JobStatus::Partial);
        assert_eq!(finished.progress.done, 1);
        assert_eq!(finished.progress.failed, 1);

        let failed = finished
            .items
            .iter()
            .find(|item| item.status == ItemStatus::Failed)
            .expect("item fallido");
        assert_eq!(failed.error_code.as_deref(), Some("SU-E161"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_is_reported_with_a_code() {
        let dir = scratch("corrupt");
        let broken = dir.join("rota.png");
        std::fs::write(&broken, b"no soy un PNG").expect("escritura");

        let manager = JobManager::new(128);
        let job = manager
            .create(request(vec![broken.display().to_string()], &dir), context(dir.clone()))
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.status, JobStatus::Failed);

        let code = finished.items[0].error_code.clone().unwrap_or_default();
        assert!(
            ["SU-E100", "SU-E101", "SU-E102"].contains(&code.as_str()),
            "codigo inesperado: {code}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelling_marks_the_remaining_items_as_skipped() {
        let dir = scratch("cancel");
        let items: Vec<String> = (0..6)
            .map(|index| {
                write_source(&dir, &format!("img{index}.png"), 96, 96)
                    .display()
                    .to_string()
            })
            .collect();

        let manager = JobManager::new(512);
        let job = manager
            .create(request(items, &dir), context(dir.clone()))
            .expect("creacion");

        // Se cancela en cuanto arranca; el resto debe quedar omitido.
        std::thread::sleep(std::time::Duration::from_millis(30));
        manager.cancel(&job.id).expect("cancelacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.status, JobStatus::Cancelled);

        let skipped = finished
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::Skipped)
            .count();
        assert!(skipped > 0, "no se marco ningun item como omitido");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pausing_and_resuming_does_not_lose_items() {
        let dir = scratch("pause");
        let items: Vec<String> = (0..3)
            .map(|index| {
                write_source(&dir, &format!("p{index}.png"), 64, 64)
                    .display()
                    .to_string()
            })
            .collect();

        let manager = JobManager::new(512);
        let job = manager
            .create(request(items, &dir), context(dir.clone()))
            .expect("creacion");

        manager.pause(&job.id).expect("pausa");
        assert_eq!(manager.get(&job.id).unwrap().status, JobStatus::Paused);

        std::thread::sleep(std::time::Duration::from_millis(80));
        manager.resume(&job.id).expect("reanudacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.status, JobStatus::Completed);
        assert_eq!(finished.progress.done, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn controlling_a_job_that_does_not_exist_fails_clearly() {
        let manager = JobManager::new(16);
        assert!(manager.pause("job-inexistente").is_err());
        assert!(manager.cancel("job-inexistente").is_err());
        assert!(manager.get("job-inexistente").is_none());
    }

    #[test]
    fn controlling_a_job_whose_thread_already_finished_is_not_an_error() {
        // La ventana de carrera que quedaba: el trabajo existe en el registro (su
        // hilo acaba de terminar, asi que ya no hay control), y llega una pausa o
        // una cancelacion. Antes el gestor devolvia un error interno y el servidor
        // lo convertia en un 500: un fallo de motor inventado para una peticion que
        // solo llego tarde. El estado esperado es `NotRunning`, sin efectos.
        let dir = scratch("late-control");
        let source = write_source(&dir, "a.png", 24, 24);
        let manager = JobManager::new(64);
        let job = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context(dir.clone()),
            )
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.status, JobStatus::Completed);

        assert_eq!(
            manager.pause(&job.id).expect("pausa tardia"),
            ControlOutcome::NotRunning
        );
        assert_eq!(
            manager.cancel(&job.id).expect("cancelacion tardia"),
            ControlOutcome::NotRunning
        );
        assert_eq!(
            manager.resume(&job.id).expect("reanudacion tardia"),
            ControlOutcome::NotRunning
        );

        // Y no se ha mentido sobre el estado del trabajo: sigue terminado, con su
        // item resuelto, no en pausa ni en ejecucion.
        let after = manager.get(&job.id).expect("el trabajo sigue existiendo");
        assert_eq!(after.status, JobStatus::Completed);
        assert_eq!(after.items[0].status, ItemStatus::Done);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pause_that_arrives_after_the_end_does_not_resurrect_the_job() {
        // El reverso del caso anterior: reanudar algo que no corre no lo relanza en
        // silencio. `resume_or_restart` si lo relanza, y lo hace por la cola; pero
        // `resume` solo quita una pausa, y aqui no hay ninguna que quitar.
        let dir = scratch("late-resume");
        let source = write_source(&dir, "b.png", 24, 24);
        let manager = JobManager::new(64);
        let job = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context(dir.clone()),
            )
            .expect("creacion");

        wait_for_finish(&manager, &job.id, 30_000);

        assert_eq!(
            manager
                .resume(&job.id)
                .expect("reanudacion sobre un trabajo terminado"),
            ControlOutcome::NotRunning
        );
        assert_eq!(
            manager.get(&job.id).expect("trabajo").status,
            JobStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn jobs_are_listed_most_recent_first() {
        let dir = scratch("list");
        let source = write_source(&dir, "x.png", 32, 32);
        let manager = JobManager::new(64);

        let first = manager
            .create(request(vec![source.display().to_string()], &dir), context(dir.clone()))
            .expect("primer trabajo");
        let second = manager
            .create(request(vec![source.display().to_string()], &dir), context(dir.clone()))
            .expect("segundo trabajo");

        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, second.id);
        assert_eq!(list[1].id, first.id);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_mode_and_scale_pair_is_rejected() {
        // Todos los pares existen, asi que se comprueba el mensaje del camino de
        // error usando un conjunto de pipelines vacio.
        let dir = scratch("nopipeline");
        let source = write_source(&dir, "x.png", 32, 32);

        let mut ctx = context(dir.clone());
        ctx.pipelines = Arc::new(PipelineSet {
            version: 1,
            pipelines: Vec::new(),
        });

        let manager = JobManager::new(16);
        let error = manager
            .create(request(vec![source.display().to_string()], &dir), ctx)
            .unwrap_err();
        assert!(error.to_string().contains("pipeline"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timestamps_are_well_formed() {
        let stamp = now_iso8601();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'));
        assert!(stamp.contains('T'));
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], "T");
    }

    #[test]
    fn file_name_handles_both_separators() {
        assert_eq!(file_name("/home/u/foto.png"), "foto.png");
        assert_eq!(file_name(r"C:\fotos\foto.png"), "foto.png");
        assert_eq!(file_name("solo.png"), "solo.png");
    }

    // -----------------------------------------------------------------------
    // Persistencia y reanudacion
    // -----------------------------------------------------------------------

    #[test]
    fn identifiers_are_unique_within_a_session() {
        let manager = JobManager::new(16);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            assert!(seen.insert(manager.next_id()), "identificador repetido");
        }
    }

    #[test]
    fn identifiers_do_not_collide_across_restarts() {
        // Dos gestores distintos simulan dos sesiones. Los identificadores llevan
        // marca de tiempo precisamente para que no puedan chocar.
        let first = JobManager::new(16).next_id();
        let second = JobManager::new(16).next_id();
        assert_ne!(first, second);
        assert!(first.starts_with("job-"));
    }

    #[test]
    fn a_finished_job_survives_a_restart() {
        let dir = scratch("persist");
        let source = write_source(&dir, "a.png", 32, 32);
        let database = dir.join("jobs.db");

        let job_id = {
            let store = JobStore::open(&database).expect("almacen");
            let manager = JobManager::with_store(store, 256);
            let job = manager
                .create(
                    request(vec![source.display().to_string()], &dir),
                    context(dir.clone()),
                )
                .expect("creacion");
            let finished = wait_for_finish(&manager, &job.id, 30_000);
            assert_eq!(finished.status, JobStatus::Completed);
            job.id
        };

        // Sesion nueva sobre la misma base de datos.
        let store = JobStore::open(&database).expect("reapertura");
        let manager = JobManager::with_store(store, 256);
        assert_eq!(manager.restore().expect("restauracion"), 0);

        let restored = manager.get(&job_id).expect("deberia estar en la lista");
        assert_eq!(restored.status, JobStatus::Completed);
        assert_eq!(restored.items.len(), 1);
        assert!(restored.items[0].out_path.is_some(), "se perdio la ruta de salida");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_interrupted_by_a_crash_comes_back_as_paused() {
        let dir = scratch("interrupted");
        let source = write_source(&dir, "a.png", 32, 32);
        let database = dir.join("jobs.db");

        let job_id = {
            let store = JobStore::open(&database).expect("almacen");
            let manager = JobManager::with_store(store, 64);
            let job = manager
                .create(
                    request(vec![source.display().to_string()], &dir),
                    context(dir.clone()),
                )
                .expect("creacion");
            wait_for_finish(&manager, &job.id, 30_000);
            job.id
        };

        // Se simula que el proceso murio a mitad: el trabajo quedo "en ejecucion".
        {
            let store = JobStore::open(&database).expect("reapertura");
            let mut stuck = store.load(&job_id).expect("lectura").expect("existe");
            stuck.status = JobStatus::Running;
            store.save(&stuck).expect("guardado");
        }

        let store = JobStore::open(&database).expect("tercera apertura");
        let manager = JobManager::with_store(store, 64);

        assert_eq!(manager.restore().expect("restauracion"), 1);
        let restored = manager.get(&job_id).expect("deberia estar");
        assert_eq!(
            restored.status,
            JobStatus::Paused,
            "un trabajo interrumpido debe ofrecerse para reanudar, no quedarse en curso"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resuming_retries_only_what_is_missing() {
        let dir = scratch("resume");
        let good = write_source(&dir, "buena.png", 32, 32);
        let broken = dir.join("rota.png");
        std::fs::write(&broken, b"no soy un PNG").expect("escritura");

        let manager = JobManager::new(256);
        let job = manager
            .create(
                request(
                    vec![good.display().to_string(), broken.display().to_string()],
                    &dir,
                ),
                context(dir.clone()),
            )
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.progress.done, 1);
        assert_eq!(finished.progress.failed, 1);
        assert_eq!(finished.status, JobStatus::Partial);

        // Al reanudar: la buena no se repite, la rota se reintenta y vuelve a
        // fallar. El recuento no debe doblarse.
        manager
            .resume_job(&job.id, context(dir.clone()))
            .expect("reanudacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.progress.done, 1, "la buena se ha reprocesado");
        assert_eq!(finished.progress.failed, 1, "el fallo se ha contado dos veces");
        assert_eq!(finished.status, JobStatus::Partial);
        assert_eq!(finished.items[0].status, ItemStatus::Done);
        assert_eq!(finished.items[1].status, ItemStatus::Failed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resuming_a_job_with_nothing_left_is_rejected() {
        let dir = scratch("resume-nada");
        let source = write_source(&dir, "a.png", 32, 32);

        let manager = JobManager::new(64);
        let job = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context(dir.clone()),
            )
            .expect("creacion");
        wait_for_finish(&manager, &job.id, 30_000);

        let error = manager
            .resume_job(&job.id, context(dir.clone()))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E001");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resuming_an_unknown_job_is_rejected() {
        let dir = scratch("resume-desconocido");
        let manager = JobManager::new(16);
        assert!(manager.resume_job("job-inexistente", context(dir.clone())).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_failure_does_not_stop_the_batch() {
        // El almacen apunta a una ruta imposible, pero el trabajo debe terminar
        // igual: perder el registro es malo, matar el lote es peor.
        let dir = scratch("store-roto");
        let source = write_source(&dir, "a.png", 32, 32);

        let store = JobStore::open_in_memory().expect("almacen en memoria");
        let manager = JobManager::with_store(store, 64);

        let job = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context(dir.clone()),
            )
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 30_000);
        assert_eq!(finished.status, JobStatus::Completed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Planificador: limite de concurrencia y prioridad
    // -----------------------------------------------------------------------

    /// Puerta que la prueba abre cuando quiere.
    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        condvar: std::sync::Condvar,
    }

    impl Gate {
        fn wait(&self) {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            while !*open {
                open = self
                    .condvar
                    .wait_timeout(open, std::time::Duration::from_millis(50))
                    .map(|(guard, _)| guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner().0);
            }
        }

        fn open(&self) {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *open = true;
            self.condvar.notify_all();
        }
    }

    /// Proveedor que se para hasta que la prueba lo suelta.
    ///
    /// Sin esto, un trabajo con una imagen de 32x32 termina antes de que la prueba
    /// pueda mirar cuantos hay esperando, y comprobar el limite de concurrencia se
    /// convierte en una carrera.
    struct GatedProvider {
        gate: Arc<Gate>,
    }

    impl BackendProvider for GatedProvider {
        fn backend_for(&self, model_id: &str) -> SuResult<Box<dyn su_inference::Backend>> {
            self.gate.wait();
            MockBackendProvider.backend_for(model_id)
        }
    }

    /// El mismo contexto, con otro proveedor.
    fn context_with(dir: PathBuf, provider: Arc<dyn BackendProvider>) -> JobContext {
        JobContext {
            provider,
            ..context(dir)
        }
    }

    /// Proveedor que se comporta como el motor con ONNX cuando el modelo que la
    /// etapa necesita **no esta descargado**: no hay backend posible para ese id.
    ///
    /// Es el caso de una instalacion nueva, no una hipotesis: el sidecar arranca
    /// con ONNX Runtime y el directorio de modelos vacio.
    struct MissingModelProvider;

    impl BackendProvider for MissingModelProvider {
        fn name(&self) -> &str {
            "sin-modelos"
        }

        fn backend_for(&self, _model_id: &str) -> SuResult<Box<dyn su_inference::Backend>> {
            Err(SuError::ModelMissing(
                "/modelos/todavia-no-descargado.onnx".to_string(),
            ))
        }
    }

    #[test]
    fn a_missing_model_is_interpolated_and_said_instead_of_failing_the_image() {
        // El fallo que esto arregla, medido: con ONNX Runtime disponible y el
        // directorio de modelos vacio, el motor no puede cargar la etapa de
        // escalado y la imagen entera se perdia. Un interpolador da un resultado
        // peor que el modelo, pero mucho mejor que un archivo que no existe; lo que
        // no puede pasar es que nadie sepa cual de los dos se uso.
        let dir = scratch("respaldo");
        let source = write_source(&dir, "a.png", 32, 32);

        let mut ctx = context_with(dir.clone(), Arc::new(MissingModelProvider));
        ctx.fallback = Some(Arc::new(MockBackendProvider));

        let manager = JobManager::new(64);
        let job = manager
            .create(request(vec![source.display().to_string()], &dir), ctx)
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 5000);

        assert_eq!(finished.status, JobStatus::Completed, "{finished:?}");
        let item = &finished.items[0];
        assert!(item.degraded, "el item uso el respaldo: tiene que decirlo");
        assert!(
            item.skipped.iter().any(|note| note.contains("respaldo")),
            "las notas tienen que nombrar el respaldo: {:?}",
            item.skipped
        );
        assert!(
            item.out_path
                .as_ref()
                .is_some_and(|path| std::path::Path::new(path).exists()),
            "tiene que quedar un archivo en la carpeta de salida"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_fallback_a_missing_model_still_fails_the_image() {
        // El respaldo es una decision de quien construye el contexto, no un
        // comportamiento escondido del runner: sin el, el error sigue siendo el
        // error, con su codigo.
        let dir = scratch("sin-respaldo");
        let source = write_source(&dir, "a.png", 32, 32);

        let manager = JobManager::new(64);
        let job = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context_with(dir.clone(), Arc::new(MissingModelProvider)),
            )
            .expect("creacion");

        let finished = wait_for_finish(&manager, &job.id, 5000);

        assert_eq!(finished.status, JobStatus::Failed);
        assert_eq!(finished.items[0].error_code.as_deref(), Some("SU-E110"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Espera a que se cumpla una condicion, en lugar de suponer que ya se cumple.
    fn wait_until(what: &str, condition: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !condition() {
            if std::time::Instant::now() > deadline {
                panic!("no se cumplio en 5 s: {what}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn the_queue_serves_the_highest_priority_first() {
        // Mas prioridad primero, sin importar el orden de llegada.
        assert_eq!(next_index(&[(1, 1), (9, 2)]), Some(1));
        assert_eq!(next_index(&[(9, 1), (1, 2)]), Some(0));
        // A igualdad de prioridad, el que llego antes.
        assert_eq!(next_index(&[(5, 7), (5, 3), (5, 9)]), Some(1));
        // Y una cola vacia no devuelve nada, en lugar de un indice inventado.
        assert_eq!(next_index(&[]), None);
    }

    #[test]
    fn a_job_waits_for_its_turn_instead_of_all_running_at_once() {
        // `concurrency` no se leia en ninguna parte: se lanzaba un hilo por trabajo
        // y el limite configurado no significaba nada.
        let dir = scratch("concurrencia");
        let source = write_source(&dir, "a.png", 32, 32);

        let gate = Arc::new(Gate::default());
        let provider = Arc::new(GatedProvider {
            gate: Arc::clone(&gate),
        });

        let manager = JobManager::new(256);

        let first = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context_with(dir.clone(), provider.clone()),
            )
            .expect("creacion");

        wait_until("el primer trabajo deberia estar corriendo", || {
            manager.running() == 1
        });

        let second = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context_with(dir.clone(), provider.clone()),
            )
            .expect("creacion");

        // El limite por defecto es uno: el segundo espera su turno.
        assert_eq!(manager.running(), 1, "solo deberia correr uno");
        assert_eq!(manager.queued(), 1, "el segundo deberia estar esperando");
        assert_eq!(
            manager.get(&second.id).map(|job| job.status),
            Some(JobStatus::Queued),
            "un trabajo que espera turno no puede decir que esta en ejecucion"
        );

        gate.open();

        assert_eq!(
            wait_for_finish(&manager, &first.id, 30_000).status,
            JobStatus::Completed
        );
        assert_eq!(
            wait_for_finish(&manager, &second.id, 30_000).status,
            JobStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn raising_the_limit_lets_a_waiting_job_start() {
        // El ajuste "imagenes simultaneas" tiene que servir para algo: al subirlo,
        // lo que estaba esperando arranca sin crear otro trabajo.
        let dir = scratch("limite");
        let source = write_source(&dir, "a.png", 32, 32);

        let gate = Arc::new(Gate::default());
        let provider = Arc::new(GatedProvider {
            gate: Arc::clone(&gate),
        });

        let manager = JobManager::new(256);

        let first = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context_with(dir.clone(), provider.clone()),
            )
            .expect("creacion");

        wait_until("el primer trabajo deberia estar corriendo", || {
            manager.running() == 1
        });

        let second = manager
            .create(
                request(vec![source.display().to_string()], &dir),
                context_with(dir.clone(), provider.clone()),
            )
            .expect("creacion");

        assert_eq!(manager.queued(), 1);

        manager.set_max_concurrent(2);

        wait_until("el segundo deberia arrancar al ampliar el limite", || {
            manager.running() == 2
        });
        assert_eq!(manager.queued(), 0);

        gate.open();

        assert_eq!(
            wait_for_finish(&manager, &first.id, 30_000).status,
            JobStatus::Completed
        );
        assert_eq!(
            wait_for_finish(&manager, &second.id, 30_000).status,
            JobStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
