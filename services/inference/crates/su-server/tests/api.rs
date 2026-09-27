//! Pruebas de integracion de la API del sidecar.
//!
//! Levantan el servidor **de verdad**, en un puerto efimero, y hablan con el por
//! un socket. Es a proposito: las pruebas unitarias del crate llaman al router en
//! memoria, asi que no ven ni el enlace del puerto, ni el fichero de puerto, ni el
//! middleware de autorizacion funcionando sobre una conexion real. Y esas tres
//! cosas son justo lo que la aplicacion usa para hablar con el motor: si el
//! fichero de puerto dejara de escribirse, la interfaz se quedaria esperando un
//! sidecar que en realidad esta escuchando.
//!
//! No se usa ningun cliente HTTP: se escribe la peticion a mano sobre un
//! `TcpStream`. Son cuatro lineas y evita arrastrar una dependencia entera al
//! workspace solo para probar.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use su_core::PipelineSet;
use su_hardware::{Capabilities, CpuInfo};
use su_inference::{MockBackendProvider, RunnerConfig};
use su_jobs::JobManager;
use su_models::ModelRegistry;
use su_server::{serve, AppState, Providers, ServerConfig};
use su_tiling::ProviderKind;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TOKEN: &str = "token-de-prueba-de-integracion";

/// Una maquina sin GPU: el caso mas simple y el que no depende del equipo donde
/// corran las pruebas.
fn capabilities() -> Capabilities {
    Capabilities {
        cpu: CpuInfo {
            brand: "CPU de prueba".to_string(),
            physical_cores: 4,
            logical_cores: 8,
        },
        gpus: Vec::new(),
        ram_total_mb: 16384,
        providers: Vec::new(),
        recommended: ProviderKind::Cpu,
        ort_version: None,
    }
}

/// Servidor escuchando, con su directorio de trabajo.
struct Harness {
    port: u16,
    dir: PathBuf,
}

impl Harness {
    /// Arranca el servidor y espera a que publique su puerto.
    ///
    /// El puerto se lee del **fichero** que escribe el servidor, igual que hace
    /// Electron. Comprobarlo aqui es parte del objetivo: es la pieza que une la
    /// aplicacion con el motor.
    async fn start(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("su-api-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("directorio de trabajo");

        let portfile = dir.join("runtime.json");

        let state = Arc::new(AppState::new(
            TOKEN,
            capabilities(),
            Arc::new(
                ModelRegistry::from_json(r#"{"manifestVersion":2,"models":[]}"#)
                    .expect("manifiesto"),
            ),
            Arc::new(PipelineSet::embedded().expect("pipelines")),
            dir.join("models"),
            Providers::new(Arc::new(MockBackendProvider), None),
            RunnerConfig::default(),
            JobManager::new(256),
        ));

        let serving = Arc::clone(&state);
        let config = ServerConfig {
            // Puerto 0: que lo elija el sistema, para no chocar con nada.
            port: 0,
            portfile: Some(portfile.clone()),
        };

        tokio::spawn(async move {
            // Un error aqui significa que el servidor no arranco; la prueba lo
            // detecta porque el fichero de puerto nunca aparece.
            let _ = serve(serving, config).await;
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        let port = loop {
            if let Ok(text) = std::fs::read_to_string(&portfile) {
                let parsed = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|json| json.get("port").and_then(|value| value.as_u64()));

                if let Some(port) = parsed {
                    break port as u16;
                }
            }

            assert!(
                Instant::now() < deadline,
                "el servidor no publico su puerto en 10 s"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        Harness { port, dir }
    }

    /// Peticion cruda. Devuelve el codigo de estado y el cuerpo.
    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .await
            .expect("conexion con el sidecar");

        let auth = match token {
            Some(token) => format!("Authorization: Bearer {token}\r\n"),
            None => String::new(),
        };

        let (content_type, payload) = match body {
            Some(body) => ("Content-Type: application/json\r\n", body),
            None => ("", ""),
        };

        let request = format!(
            "{method} {path} HTTP/1.1\r\n\
             Host: 127.0.0.1\r\n\
             {auth}\
             {content_type}\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n\
             {payload}",
            payload.len()
        );

        stream
            .write_all(request.as_bytes())
            .await
            .expect("escritura de la peticion");

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .expect("lectura de la respuesta");

        let text = String::from_utf8_lossy(&raw).into_owned();

        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .expect("codigo de estado en la respuesta");

        // `Connection: close` hace que el servidor cierre al responder, asi que
        // `read_to_end` termina. El cuerpo va despues de la primera linea en
        // blanco.
        let body = text
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or_default();

        (status, body)
    }

    async fn get(&self, path: &str) -> (u16, String) {
        self.request("GET", path, Some(TOKEN), None).await
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// El JSON de una peticion de trabajo valida, con el elemento que se le pase.
fn job_payload(item: &str) -> String {
    format!(
        r#"{{
            "mode": "photo",
            "scale": 4,
            "items": ["{item}"],
            "output": {{ "dir": "/tmp/out", "format": "png", "quality": 95 }},
            "options": {{ "tileSize": "auto", "device": "auto", "concurrency": 1,
                          "unloadBetweenImages": false, "modelChainMode": "auto",
                          "faceRestore": "off", "denoise": "off", "sharpen": false }}
        }}"#
    )
}

#[tokio::test]
async fn the_health_endpoint_answers_without_a_token() {
    // Es la unica ruta abierta, y tiene que serlo: la aplicacion la usa para
    // saber si el motor esta vivo antes de tener el token a mano.
    let harness = Harness::start("salud").await;

    let (status, body) = harness.request("GET", "/v1/health", None, None).await;
    assert_eq!(status, 200, "cuerpo: {body}");
    assert!(body.contains("ok"), "cuerpo: {body}");
}

#[tokio::test]
async fn every_other_endpoint_requires_the_token() {
    let harness = Harness::start("token").await;

    for path in ["/v1/capabilities", "/v1/models", "/v1/pipelines", "/v1/jobs"] {
        let (status, _) = harness.request("GET", path, None, None).await;
        assert_eq!(status, 401, "{path} deberia exigir el token");

        let (status, _) = harness
            .request("GET", path, Some("token-equivocado"), None)
            .await;
        assert_eq!(status, 401, "{path} no deberia aceptar un token equivocado");

        let (status, _) = harness.request("GET", path, Some(TOKEN), None).await;
        assert_eq!(status, 200, "{path} deberia aceptar el token correcto");
    }
}

#[tokio::test]
async fn capabilities_describes_the_machine() {
    let harness = Harness::start("capacidades").await;

    let (status, body) = harness.get("/v1/capabilities").await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).expect("JSON de capacidades");
    assert_eq!(json["cpu"]["physicalCores"], 4);
    assert_eq!(json["ramTotalMb"], 16384);
    // El EP recomendado tiene que viajar: es lo que la interfaz muestra.
    assert_eq!(json["recommended"], "cpu");
}

#[tokio::test]
async fn models_reports_the_directory_the_application_must_write_to() {
    // `modelsDir` lo resuelve el sidecar y lo publica; la aplicacion no puede
    // deducirlo por su cuenta o las dos rutas se separarian (ADR-019).
    let harness = Harness::start("modelos").await;

    let (status, body) = harness.get("/v1/models").await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).expect("JSON de modelos");
    let dir = json["modelsDir"].as_str().expect("modelsDir");

    assert!(
        dir.ends_with("models"),
        "el directorio deberia acabar en 'models': {dir}"
    );
    assert!(json["models"].is_array());
}

#[tokio::test]
async fn pipelines_lists_the_two_modes() {
    let harness = Harness::start("pipelines").await;

    let (status, body) = harness.get("/v1/pipelines").await;
    assert_eq!(status, 200);
    assert!(body.contains("photo"), "cuerpo: {body}");
    assert!(body.contains("illustration"), "cuerpo: {body}");
}

#[tokio::test]
async fn a_job_can_be_created_and_then_listed() {
    // El recorrido que hace la interfaz al pulsar Upscaly: crear el trabajo y
    // verlo en la cola.
    let harness = Harness::start("trabajos").await;

    let payload = job_payload("/tmp/no-existe-pero-da-igual.png");

    let (status, body) = harness
        .request("POST", "/v1/jobs", Some(TOKEN), Some(&payload))
        .await;
    // 201 y no 200: la peticion crea un recurso. El cliente comprueba `response.ok`,
    // asi que cualquier 2xx vale; lo que no vale es no fijar cual se devuelve.
    assert_eq!(status, 201, "cuerpo: {body}");

    let created: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
    let id = created["id"].as_str().expect("id del trabajo").to_string();
    assert!(!id.is_empty());
    assert_eq!(created["mode"], "photo");

    let (status, body) = harness.get("/v1/jobs").await;
    assert_eq!(status, 200);

    let list: serde_json::Value = serde_json::from_str(&body).expect("lista de trabajos");
    let jobs = list.as_array().expect("la lista deberia ser un array");

    assert!(
        jobs.iter().any(|job| job["id"] == id.as_str()),
        "el trabajo recien creado deberia estar en la lista: {body}"
    );
}

#[tokio::test]
async fn an_empty_job_is_rejected_with_its_code() {
    // Un trabajo sin imagenes no puede colarse: el motor lo rechaza con SU-E001.
    let harness = Harness::start("vacio").await;

    let payload = r#"{
        "mode": "photo",
        "scale": 4,
        "items": [],
        "output": { "dir": "/tmp/out", "format": "png", "quality": 95 },
        "options": { "tileSize": "auto", "device": "auto", "concurrency": 1,
                     "unloadBetweenImages": false, "modelChainMode": "auto",
                     "faceRestore": "off", "denoise": "off", "sharpen": false }
    }"#;

    let (status, body) = harness
        .request("POST", "/v1/jobs", Some(TOKEN), Some(payload))
        .await;

    assert_ne!(status, 200, "no deberia aceptarlo");
    assert!(
        body.contains("SU-E001"),
        "el codigo de error deberia viajar hasta el cliente: {body}"
    );
}

#[tokio::test]
async fn an_unknown_job_is_a_404() {
    let harness = Harness::start("404").await;

    let (status, body) = harness.get("/v1/jobs/no-existe").await;
    assert_eq!(status, 404);
    // Y con codigo: lo contesta el manejador, no el enrutador. Ver la prueba
    // siguiente, que es la que distingue las dos cosas.
    assert!(body.contains("SU-E404"), "cuerpo: {body}");
}

#[tokio::test]
#[allow(clippy::similar_names)]
async fn the_job_id_routes_reach_the_handler_instead_of_the_router() {
    // Un 404 puede venir de dos sitios muy distintos: del enrutador, porque la
    // ruta no coincide con nada, o del manejador, porque el trabajo no existe. El
    // primero es un fallo de programacion y el segundo es una respuesta correcta,
    // y los dos se ven igual desde fuera si solo se mira el codigo de estado.
    //
    // Esto no es teorico: las rutas se declararon con la sintaxis `{id}` de axum
    // 0.8 sobre axum 0.7, donde el parametro se escribe `:id`. El segmento se tomo
    // como texto literal, asi que **ninguna** peticion con un id de verdad
    // coincidia: consultar, pausar, reanudar y cancelar un trabajo respondian 404
    // siempre, con el trabajo creado y corriendo. La prueba que habia aqui
    // comprobaba un 404 con un id inventado y pasaba por el motivo equivocado.
    let harness = Harness::start("rutas-id").await;

    // Consultar un trabajo que no existe: lo contesta el manejador, con codigo.
    let (status, body) = harness.get("/v1/jobs/no-existe").await;
    assert_eq!(status, 404, "cuerpo: {body}");
    assert!(
        body.contains("SU-E404"),
        "deberia contestarlo el manejador (cuerpo con codigo), y contesto: {body:?}"
    );

    // Las tres acciones son POST. Pedirlas con GET tiene que dar 405: la ruta
    // existe y lo unico que no cuadra es el metodo. Con el parametro declarado mal,
    // la respuesta habria sido un 404 de enrutador.
    for path in [
        "/v1/jobs/no-existe/pause",
        "/v1/jobs/no-existe/resume",
        "/v1/jobs/no-existe/cancel",
    ] {
        let (status, body) = harness.get(path).await;
        assert_eq!(
            status, 405,
            "{path} deberia ser una ruta POST existente, y contesto {status}: {body:?}"
        );
    }

    // Control: una ruta que de verdad no existe la contesta el enrutador, sin
    // cuerpo. Si esto dejara de ser cierto, la comprobacion anterior no valdria.
    let (status, body) = harness.get("/v1/no-existe-en-absoluto").await;
    assert_eq!(status, 404);
    assert!(
        !body.contains("SU-E404"),
        "una ruta inexistente no deberia contestar con el error del dominio: {body:?}"
    );
}

#[tokio::test]
async fn a_created_job_can_be_fetched_paused_resumed_and_cancelled_by_its_id() {
    // El recorrido completo de la interfaz sobre un trabajo concreto. Es la
    // prueba que faltaba: se creaba un trabajo y se listaba la cola, pero **nunca
    // se pedia uno por su identificador**, que es justo lo que hace la interfaz
    // para refrescar el progreso y para los botones de pausa, reanudar y cancelar.
    let harness = Harness::start("por-id").await;

    let payload = job_payload("/tmp/no-existe-pero-da-igual.png");
    let (status, body) = harness
        .request("POST", "/v1/jobs", Some(TOKEN), Some(&payload))
        .await;
    assert_eq!(status, 201, "cuerpo: {body}");

    let created: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
    let id = created["id"].as_str().expect("id del trabajo").to_string();

    let (status, body) = harness.get(&format!("/v1/jobs/{id}")).await;
    assert_eq!(status, 200, "consultar el trabajo recien creado: {body}");
    let fetched: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
    assert_eq!(fetched["id"], id.as_str());

    // Las tres acciones de control tienen que llegar al manejador y contestar con
    // el trabajo, sin depender de en que punto del procesamiento se pillen: el
    // trabajo se resuelve en milisegundos porque su imagen no existe, y pausar algo
    // que acaba de terminar es una peticion que llego tarde, no un fallo del motor.
    for accion in ["pause", "resume", "cancel"] {
        let (status, body) = harness
            .request("POST", &format!("/v1/jobs/{id}/{accion}"), Some(TOKEN), Some("{}"))
            .await;
        assert_eq!(
            status, 200,
            "{accion} deberia contestar con el trabajo, no con un error: {body}"
        );

        let json: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
        assert_eq!(
            json["id"], id.as_str(),
            "{accion} deberia devolver el mismo trabajo: {body}"
        );
    }
}

#[tokio::test]
async fn pausing_at_the_finish_line_never_reports_an_engine_failure() {
    // La carrera de verdad, buscada a proposito: un trabajo de un solo item que se
    // resuelve en milisegundos (su imagen no existe) y una pausa disparada sin
    // esperar. El resultado depende de quien llegue antes —puede pausarse en
    // marcha, o llegar despues—, y las dos cosas son legitimas; lo que no lo es,
    // es responder un 500 porque el trabajo terminara entre la comprobacion y la
    // accion. Se repite para dejar la ventana a la vista en varias ejecuciones.
    let harness = Harness::start("carrera").await;

    for intento in 0..25 {
        let payload = job_payload("/tmp/no-existe-pero-da-igual.png");
        let (status, body) = harness
            .request("POST", "/v1/jobs", Some(TOKEN), Some(&payload))
            .await;
        assert_eq!(status, 201, "intento {intento}, cuerpo: {body}");
        let created: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
        let id = created["id"].as_str().expect("id del trabajo").to_string();

        for accion in ["pause", "cancel"] {
            let (status, body) = harness
                .request("POST", &format!("/v1/jobs/{id}/{accion}"), Some(TOKEN), Some("{}"))
                .await;
            assert_eq!(
                status, 200,
                "intento {intento}: {accion} respondio {status} en vez del trabajo: {body}"
            );
        }
    }
}

#[tokio::test]
async fn controlling_a_job_that_already_finished_is_not_an_engine_failure() {
    // El caso real, no el teorico: el usuario pulsa pausa o cancelar unas decimas
    // de segundo despues de que el trabajo termine por su cuenta. Antes respondia
    // 500 con "no hay ningun trabajo activo", que es un mensaje de motor roto para
    // una peticion que simplemente llego tarde.
    let harness = Harness::start("tarde").await;

    let payload = job_payload("/tmp/no-existe-pero-da-igual.png");
    let (status, body) = harness
        .request("POST", "/v1/jobs", Some(TOKEN), Some(&payload))
        .await;
    assert_eq!(status, 201, "cuerpo: {body}");
    let created: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
    let id = created["id"].as_str().expect("id del trabajo").to_string();

    // Se espera a que deje de estar en curso, en lugar de suponer cuanto tarda.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (_, body) = harness.get(&format!("/v1/jobs/{id}")).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("JSON del trabajo");
        let estado = json["status"].as_str().unwrap_or_default().to_string();

        if !matches!(estado.as_str(), "queued" | "running") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "el trabajo no termino en 10 s (estado {estado})"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    for accion in ["pause", "cancel"] {
        let (status, body) = harness
            .request("POST", &format!("/v1/jobs/{id}/{accion}"), Some(TOKEN), Some("{}"))
            .await;
        assert_eq!(
            status, 200,
            "{accion} sobre un trabajo terminado deberia ser 200, no un error del motor: {body}"
        );
    }

    // Y un id que no existe sigue siendo 404, para que las dos cosas no se
    // confundan en la direccion contraria.
    let (status, body) = harness
        .request("POST", "/v1/jobs/no-existe/pause", Some(TOKEN), Some("{}"))
        .await;
    assert_eq!(status, 404, "cuerpo: {body}");
    assert!(body.contains("SU-E404"), "cuerpo: {body}");
}
