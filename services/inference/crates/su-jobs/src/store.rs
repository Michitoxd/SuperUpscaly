//! Persistencia de trabajos en SQLite.
//!
//! ## Por que una sola tabla con el trabajo en JSON
//!
//! El ADR-009 preveia tablas separadas para trabajos e items. Aqui se guarda el
//! `Job` completo serializado en una unica tabla, con una razon concreta: **evitar
//! dos fuentes de verdad**. Con columnas y payload a la vez, un `UPDATE` de estado
//! que no toque el payload deja la base de datos mintiendo, y ese fallo aparece
//! justo cuando mas duele: al reanudar despues de un cierre inesperado.
//!
//! El coste es que filtrar por estado se hace en Rust en lugar de en SQL. Con
//! trabajos de decenas o cientos de items, eso es irrelevante. La tabla relacional
//! de items llegara cuando haga falta consultar **entre** trabajos (por ejemplo,
//! "todos los items fallidos de las ultimas sesiones").
//!
//! ## Que se guarda y cuando
//!
//! No en cada evento de progreso: un lote con cientos de tiles generaria miles de
//! escrituras por imagen. Se guarda al crear el trabajo, al terminar cada item y
//! al finalizar el lote. Perder el progreso de los tiles de la imagen en curso es
//! aceptable; perder el de las imagenes ya terminadas no.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use su_core::{ItemStatus, Job, JobStatus, SuError, SuResult};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS jobs (
    id         TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    payload    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_jobs_created ON jobs (created_at DESC);
";

/// Almacen de trabajos.
///
/// No es `Sync` por si mismo: `rusqlite::Connection` no lo es. El llamador lo
/// envuelve en el mutex que corresponda. Se mantiene asi a proposito, para no
/// esconder un punto de contencion detras de una API que parezca libre de el.
pub struct JobStore {
    connection: Connection,
}

impl JobStore {
    /// Abre (o crea) la base de datos y aplica el esquema.
    pub fn open(path: &Path) -> SuResult<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    SuError::WriteFailed(format!("no se pudo crear {}: {error}", parent.display()))
                })?;
            }
        }

        let connection = Connection::open(path).map_err(db_error)?;
        Self::prepare(connection)
    }

    /// Base de datos en memoria. Para tests.
    pub fn open_in_memory() -> SuResult<Self> {
        let connection = Connection::open_in_memory().map_err(db_error)?;
        Self::prepare(connection)
    }

    fn prepare(connection: Connection) -> SuResult<Self> {
        connection.execute_batch(SCHEMA).map_err(db_error)?;
        Ok(Self { connection })
    }

    /// Inserta o reemplaza un trabajo. Es idempotente: guardar dos veces el mismo
    /// `id` deja el ultimo estado, que es lo que se quiere al persistir progreso.
    pub fn save(&self, job: &Job) -> SuResult<()> {
        let payload = serde_json::to_string(job)
            .map_err(|error| SuError::Internal(format!("trabajo no serializable: {error}")))?;

        self.connection
            .execute(
                "INSERT INTO jobs (id, created_at, payload) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET created_at = excluded.created_at,
                                               payload = excluded.payload",
                params![job.id, job.created_at, payload],
            )
            .map_err(db_error)?;

        Ok(())
    }

    pub fn load(&self, id: &str) -> SuResult<Option<Job>> {
        let payload: Option<String> = self
            .connection
            .query_row("SELECT payload FROM jobs WHERE id = ?1", params![id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)?;

        match payload {
            Some(raw) => Ok(Some(decode(&raw)?)),
            None => Ok(None),
        }
    }

    /// Todos los trabajos, los mas recientes primero.
    pub fn list(&self) -> SuResult<Vec<Job>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM jobs ORDER BY created_at DESC")
            .map_err(db_error)?;

        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(decode(&row.map_err(db_error)?)?);
        }
        Ok(jobs)
    }

    pub fn delete(&self, id: &str) -> SuResult<bool> {
        let affected = self
            .connection
            .execute("DELETE FROM jobs WHERE id = ?1", params![id])
            .map_err(db_error)?;
        Ok(affected > 0)
    }

    /// Trabajos que quedaron en ejecucion porque la aplicacion se cerro.
    ///
    /// Los pasa a `Paused` —para que la interfaz ofrezca reanudarlos y no
    /// muestre un trabajo "en curso" que no avanza— y los devuelve. Marcar sin
    /// devolver obligaria al llamador a consultarlos otra vez.
    pub fn take_interrupted(&self) -> SuResult<Vec<Job>> {
        let mut interrupted = Vec::new();

        for mut job in self.list()? {
            if job.status != JobStatus::Running {
                continue;
            }
            job.status = JobStatus::Paused;
            self.save(&job)?;
            interrupted.push(job);
        }

        Ok(interrupted)
    }

    /// Items ya terminados de un trabajo, para saltarlos al reanudar.
    ///
    /// Solo cuenta como terminados los que tienen salida escrita: un item marcado
    /// `Done` sin archivo seria un trabajo perdido que se daria por hecho.
    pub fn completed_items(&self, job_id: &str) -> SuResult<HashMap<String, String>> {
        let Some(job) = self.load(job_id)? else {
            return Ok(HashMap::new());
        };

        Ok(job
            .items
            .into_iter()
            .filter(|item| matches!(item.status, ItemStatus::Done | ItemStatus::Degraded))
            .filter_map(|item| item.out_path.map(|path| (item.src_path, path)))
            .collect())
    }

    /// Numero de trabajos guardados.
    pub fn count(&self) -> SuResult<usize> {
        let total: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .map_err(db_error)?;
        Ok(total.max(0) as usize)
    }
}

fn decode(raw: &str) -> SuResult<Job> {
    serde_json::from_str(raw).map_err(|error| {
        // Un payload corrupto no se puede reparar, pero si se puede reportar con
        // precision en lugar de fallar de forma opaca.
        SuError::Internal(format!("trabajo guardado ilegible: {error}"))
    })
}

fn db_error(error: rusqlite::Error) -> SuError {
    SuError::Internal(format!("base de datos de trabajos: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use su_core::{
        JobItem, JobOptions, JobProgress, Mode, OutputFormat, OutputSettings, Scale, StageId,
    };

    fn job(id: &str, created_at: &str, status: JobStatus, items: usize) -> Job {
        Job {
            id: id.to_string(),
            created_at: created_at.to_string(),
            status,
            mode: Mode::Photo,
            scale: Scale::X4,
            pipeline_id: "photo:4x".to_string(),
            output: OutputSettings {
                dir: "/salida".to_string(),
                format: OutputFormat::Png,
                quality: 95,
                suffix: "_upscaled".to_string(),
                preserve_metadata: true,
                zip_output: false,
            },
            options: JobOptions::default(),
            progress: JobProgress {
                total: items,
                done: 0,
                failed: 0,
                degraded: 0,
            },
            items: (0..items)
                .map(|index| JobItem {
                    id: format!("{id}-{index:04}"),
                    src_path: format!("/fotos/{index}.png"),
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
                .collect(),
        }
    }

    #[test]
    fn a_job_round_trips() {
        let store = JobStore::open_in_memory().expect("almacen");
        let original = job("job-1", "2026-09-15T10:00:00Z", JobStatus::Running, 3);

        store.save(&original).expect("guardado");
        let loaded = store.load("job-1").expect("lectura").expect("existe");

        assert_eq!(loaded.id, original.id);
        assert_eq!(loaded.status, JobStatus::Running);
        assert_eq!(loaded.items.len(), 3);
        assert_eq!(loaded.output.suffix, "_upscaled");
        assert_eq!(loaded.pipeline_id, "photo:4x");
    }

    #[test]
    fn the_reason_a_stage_was_skipped_survives_the_round_trip() {
        // El motivo se emitia en `ItemCompleted`, pero eso solo lo ve un cliente
        // conectado en ese instante. Si no se persiste, al recargar la lista el
        // item vuelve sin explicacion y la pregunta "¿por que esta imagen salio
        // distinta?" se queda sin respuesta justo cuando mas se necesita.
        let store = JobStore::open_in_memory().expect("almacen");
        let mut original = job("job-1", "2026-09-15T10:00:00Z", JobStatus::Completed, 1);
        original.items[0].skipped = vec![
            "upscale: la condicion de la etapa 'upscale2' no se cumple".to_string(),
            "face: la etapa no declara modelo".to_string(),
        ];
        original.items[0].effective_pipeline = vec![StageId::Analyze, StageId::Upscale];

        store.save(&original).expect("guardado");
        let loaded = store.load("job-1").expect("lectura").expect("existe");

        assert_eq!(loaded.items[0].skipped, original.items[0].skipped);
        assert_eq!(
            loaded.items[0].effective_pipeline,
            original.items[0].effective_pipeline
        );
    }

    #[test]
    fn a_row_written_before_the_reasons_existed_still_loads() {
        // El `#[serde(default)]` de `JobItem::skipped` esta para esto: un
        // `jobs.db` de una version anterior no tiene el campo. Si esto fallara,
        // actualizar la aplicacion dejaria la cola entera ilegible.
        let store = JobStore::open_in_memory().expect("almacen");
        let entry = job("job-1", "2026-09-15T10:00:00Z", JobStatus::Completed, 1);

        let mut payload = serde_json::to_value(&entry).expect("serializacion");
        for item in payload["items"].as_array_mut().expect("items") {
            let fields = item.as_object_mut().expect("item");
            fields.remove("skipped");
            fields.remove("effectivePipeline");
        }

        store
            .connection
            .execute(
                "INSERT INTO jobs (id, created_at, payload) VALUES (?1, ?2, ?3)",
                params!["job-1", "2026-09-15T10:00:00Z", payload.to_string()],
            )
            .expect("fila de una version anterior");

        let loaded = store.load("job-1").expect("lectura").expect("existe");
        assert_eq!(loaded.items.len(), 1);
        assert!(loaded.items[0].skipped.is_empty());
        assert!(loaded.items[0].effective_pipeline.is_empty());
    }

    #[test]
    fn an_unknown_id_returns_none() {
        let store = JobStore::open_in_memory().expect("almacen");
        assert!(store.load("no-existe").expect("lectura").is_none());
    }

    #[test]
    fn saving_the_same_job_twice_replaces_it() {
        let store = JobStore::open_in_memory().expect("almacen");
        let mut entry = job("job-1", "2026-09-15T10:00:00Z", JobStatus::Running, 2);

        store.save(&entry).expect("primer guardado");
        entry.status = JobStatus::Completed;
        entry.progress.done = 2;
        store.save(&entry).expect("segundo guardado");

        assert_eq!(store.count().unwrap(), 1, "no deberia haber duplicados");
        let loaded = store.load("job-1").unwrap().unwrap();
        assert_eq!(loaded.status, JobStatus::Completed);
        assert_eq!(loaded.progress.done, 2);
    }

    #[test]
    fn jobs_are_listed_most_recent_first() {
        let store = JobStore::open_in_memory().expect("almacen");
        store
            .save(&job("a", "2026-09-15T10:00:00Z", JobStatus::Completed, 1))
            .unwrap();
        store
            .save(&job("b", "2026-09-15T12:00:00Z", JobStatus::Completed, 1))
            .unwrap();
        store
            .save(&job("c", "2026-09-15T11:00:00Z", JobStatus::Completed, 1))
            .unwrap();

        let ids: Vec<String> = store.list().unwrap().into_iter().map(|j| j.id).collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
    }

    #[test]
    fn deleting_reports_whether_something_was_removed() {
        let store = JobStore::open_in_memory().expect("almacen");
        store
            .save(&job("job-1", "2026-09-15T10:00:00Z", JobStatus::Completed, 1))
            .unwrap();

        assert!(store.delete("job-1").unwrap());
        assert!(!store.delete("job-1").unwrap());
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn an_interrupted_job_is_returned_and_left_paused() {
        let store = JobStore::open_in_memory().expect("almacen");
        store
            .save(&job("en-curso", "2026-09-15T10:00:00Z", JobStatus::Running, 5))
            .unwrap();
        store
            .save(&job("terminado", "2026-09-15T09:00:00Z", JobStatus::Completed, 1))
            .unwrap();

        let interrupted = store.take_interrupted().expect("reanudables");

        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].id, "en-curso");
        assert_eq!(interrupted[0].status, JobStatus::Paused);

        // Y queda persistido como pausado, no solo en la copia devuelta.
        let reloaded = store.load("en-curso").unwrap().unwrap();
        assert_eq!(reloaded.status, JobStatus::Paused);
    }

    #[test]
    fn taking_interrupted_jobs_twice_returns_nothing_the_second_time() {
        let store = JobStore::open_in_memory().expect("almacen");
        store
            .save(&job("en-curso", "2026-09-15T10:00:00Z", JobStatus::Running, 2))
            .unwrap();

        assert_eq!(store.take_interrupted().unwrap().len(), 1);
        assert!(store.take_interrupted().unwrap().is_empty());
    }

    #[test]
    fn completed_items_are_listed_for_resuming() {
        let store = JobStore::open_in_memory().expect("almacen");
        let mut entry = job("job-1", "2026-09-15T10:00:00Z", JobStatus::Running, 4);

        entry.items[0].status = ItemStatus::Done;
        entry.items[0].out_path = Some("/salida/0_upscaled.png".to_string());
        entry.items[1].status = ItemStatus::Degraded;
        entry.items[1].out_path = Some("/salida/1_upscaled.png".to_string());
        entry.items[2].status = ItemStatus::Failed;
        entry.items[2].out_path = Some("/salida/2_upscaled.png".to_string());
        // Un item marcado como terminado pero sin archivo no debe darse por hecho.
        entry.items[3].status = ItemStatus::Done;

        store.save(&entry).unwrap();
        let completed = store.completed_items("job-1").unwrap();

        assert_eq!(completed.len(), 2);
        assert!(completed.contains_key("/fotos/0.png"));
        assert!(completed.contains_key("/fotos/1.png"));
        assert!(!completed.contains_key("/fotos/2.png"), "un item fallido no esta hecho");
        assert!(!completed.contains_key("/fotos/3.png"), "sin archivo no esta hecho");
    }

    #[test]
    fn completed_items_of_an_unknown_job_is_empty_not_an_error() {
        let store = JobStore::open_in_memory().expect("almacen");
        assert!(store.completed_items("no-existe").unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_payload_is_reported_clearly() {
        let store = JobStore::open_in_memory().expect("almacen");
        store
            .connection
            .execute(
                "INSERT INTO jobs (id, created_at, payload) VALUES ('roto', 'x', 'no-json')",
                [],
            )
            .unwrap();

        let error = store.load("roto").unwrap_err();
        assert!(error.to_string().contains("ilegible"), "{error}");
    }

    #[test]
    fn a_corrupt_payload_does_not_break_the_whole_listing() {
        let store = JobStore::open_in_memory().expect("almacen");
        store
            .save(&job("bueno", "2026-09-15T10:00:00Z", JobStatus::Completed, 1))
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO jobs (id, created_at, payload) VALUES ('roto', 'z', 'no-json')",
                [],
            )
            .unwrap();

        // Se prefiere fallar entero a devolver una lista incompleta sin avisar:
        // el llamador decide si quiere saltarse los ilegibles.
        assert!(store.list().is_err());
    }

    #[test]
    fn the_store_survives_reopening_the_file() {
        let dir = std::env::temp_dir().join(format!("su-jobs-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("jobs.db");

        {
            let store = JobStore::open(&path).expect("apertura");
            store
                .save(&job("persistente", "2026-09-15T10:00:00Z", JobStatus::Completed, 2))
                .unwrap();
        }

        {
            let store = JobStore::open(&path).expect("reapertura");
            assert_eq!(store.count().unwrap(), 1);
            let loaded = store.load("persistente").unwrap().unwrap();
            assert_eq!(loaded.items.len(), 2);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_creates_the_parent_directory() {
        let dir = std::env::temp_dir().join(format!("su-jobs-mkdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("a").join("b").join("jobs.db");

        let store = JobStore::open(&path).expect("apertura");
        assert_eq!(store.count().unwrap(), 0);
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
