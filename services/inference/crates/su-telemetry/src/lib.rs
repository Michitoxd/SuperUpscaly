//! # su-telemetry
//!
//! Logs estructurados en JSON con **enmascarado del directorio personal**.
//!
//! Los logs se adjuntan a los informes de error, asi que no pueden revelar la
//! estructura de carpetas del usuario. El enmascarado se hace en el propio
//! escritor, no en cada llamada: asi es imposible olvidarse en un sitio.
//!
//! La telemetria remota es opt-in y vive en otro modulo (Fase 5). Aqui solo hay
//! registro local.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use su_core::{SuError, SuResult};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Marca que sustituye al directorio personal en los logs.
pub const HOME_PLACEHOLDER: &str = "<home>";

/// Escritor que enmascara el directorio personal antes de volcar la linea.
pub struct MaskingFileWriter {
    file: Mutex<File>,
    home: String,
}

impl MaskingFileWriter {
    pub fn new(path: &Path, home: impl Into<String>) -> SuResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                SuError::WriteFailed(format!("no se pudo crear {}: {error}", parent.display()))
            })?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| {
                SuError::WriteFailed(format!("no se pudo abrir {}: {error}", path.display()))
            })?;

        Ok(Self {
            file: Mutex::new(file),
            home: home.into(),
        })
    }

    /// Sustituye cada aparicion del directorio personal. Se aplica tanto a la
    /// forma nativa como a la normalizada con barras invertidas, porque en
    /// Windows los logs mezclan las dos.
    pub fn mask(&self, text: &str) -> String {
        mask_home(text, &self.home)
    }
}

/// Logica de enmascarado, aislada para poder testearla sin tocar el disco.
///
/// Se cubren las dos formas del separador: en Windows los logs mezclan rutas con
/// `\` y con `/` segun quien las escriba.
pub fn mask_home(text: &str, home: &str) -> String {
    if home.is_empty() {
        return text.to_string();
    }

    let mut masked = text.replace(home, HOME_PLACEHOLDER);

    let forward = home.replace('\\', "/");
    if forward != home {
        masked = masked.replace(&forward, HOME_PLACEHOLDER);
    }

    let back = home.replace('/', "\\");
    if back != home {
        masked = masked.replace(&back, HOME_PLACEHOLDER);
    }

    masked
}

pub struct MaskingGuard<'a> {
    writer: &'a MaskingFileWriter,
}

impl Write for MaskingGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let masked = self.writer.mask(&text);

        // Un fallo escribiendo el log no puede tumbar la aplicacion: el
        // diagnostico es importante, pero no mas que el trabajo del usuario.
        match self.writer.file.lock() {
            Ok(mut file) => {
                let _ = file.write_all(masked.as_bytes());
                let _ = file.flush();
            }
            Err(poisoned) => {
                let mut file = poisoned.into_inner();
                let _ = file.write_all(masked.as_bytes());
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.writer.file.lock() {
            Ok(mut file) => file.flush(),
            Err(poisoned) => poisoned.into_inner().flush(),
        }
    }
}

impl<'a> MakeWriter<'a> for MaskingFileWriter {
    type Writer = MaskingGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        MaskingGuard { writer: self }
    }
}

/// Nombre de archivo del dia, con rotacion diaria.
pub fn daily_log_path(log_dir: &Path, date: &str) -> PathBuf {
    log_dir.join(format!("su-server-{date}.log"))
}

/// Fecha en formato `YYYY-MM-DD` a partir de segundos UNIX.
///
/// Se calcula a mano para no arrastrar una dependencia de fechas: es la unica
/// cosa que se necesita del calendario.
pub fn date_from_unix_seconds(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Algoritmo de Howard Hinnant: dias desde la epoca -> fecha civil.
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

/// Instala el subscriber. Devuelve la ruta del log activo.
/// Instala el registro en archivo y devuelve la ruta del log del dia.
///
/// Es **idempotente**: llamarla cuando ya hay un registro global instalado no es
/// un fallo de arranque. `try_init` devuelve error en ese caso, y tratarlo como
/// error hacia que una segunda llamada (un test de integracion que entra por
/// `main`, el servidor y el CLI en el mismo proceso) anunciara "aviso: sin
/// registro en archivo" cuando el registro estaba funcionando perfectamente.
///
/// La ruta que se devuelve sigue siendo valida: es la del archivo del dia, que
/// sera el que use quien haya instalado el registro antes.
pub fn init(log_dir: &Path, home: &str, level: &str) -> SuResult<PathBuf> {
    let date = date_from_unix_seconds(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0),
    );
    let path = daily_log_path(log_dir, &date);

    // Se pregunta antes de tocar el disco: si el registro ya esta instalado, esta
    // llamada no tiene nada que hacer ni nada que pueda fallar.
    if tracing::dispatcher::has_been_set() {
        return Ok(path);
    }

    let writer = MaskingFileWriter::new(&path, home)?;
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_current_span(false)
        .with_span_list(false);

    match tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .try_init()
    {
        Ok(()) => Ok(path),
        // Otra parte del proceso se adelanto entre la comprobacion y la
        // instalacion. Tampoco es un fallo: hay registro, solo que no lo puso
        // esta llamada.
        Err(_) if tracing::dispatcher::has_been_set() => Ok(path),
        Err(error) => Err(SuError::Internal(format!(
            "no se pudo instalar el logger: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_directory_is_masked() {
        let masked = mask_home("/home/michito/fotos/retrato.png", "/home/michito");
        assert_eq!(masked, "<home>/fotos/retrato.png");
    }

    #[test]
    fn every_occurrence_is_masked() {
        let masked = mask_home(
            "/home/michito/a -> /home/michito/b -> /home/michito/c",
            "/home/michito",
        );
        assert!(!masked.contains("/home/michito"));
        assert_eq!(masked.matches(HOME_PLACEHOLDER).count(), 3);
    }

    #[test]
    fn windows_separators_are_masked_too() {
        let masked = mask_home(r"C:\Users\michito\fotos\a.png", r"C:\Users\michito");
        assert_eq!(masked, r"<home>\fotos\a.png");
    }

    #[test]
    fn an_empty_home_leaves_the_text_untouched() {
        assert_eq!(mask_home("/tmp/a.png", ""), "/tmp/a.png");
    }

    #[test]
    fn text_without_the_home_is_untouched() {
        let text = "modelo cargado en 412 ms";
        assert_eq!(mask_home(text, "/home/michito"), text);
    }

    #[test]
    fn dates_are_computed_correctly() {
        assert_eq!(date_from_unix_seconds(0), "1970-01-01");
        // 2026-09-15T00:00:00Z
        assert_eq!(date_from_unix_seconds(1_789_430_400), "2026-09-15");
        // Un dia antes, para comprobar el cambio de dia.
        assert_eq!(date_from_unix_seconds(1_789_430_400 - 1), "2026-09-14");
    }

    #[test]
    fn leap_years_are_handled() {
        // 2024-02-29T12:00:00Z
        assert_eq!(date_from_unix_seconds(1_709_208_000), "2024-02-29");
        // 2000 fue bisiesto (divisible por 400).
        assert_eq!(date_from_unix_seconds(951_782_400), "2000-02-29");
    }

    #[test]
    fn the_writer_creates_the_directory_and_masks_what_it_writes() {
        // Directorio propio en lugar de `tempfile`: solo hace falta crear una
        // carpeta, y `tempfile` arrastraria `getrandom` y `windows-sys`.
        let dir = std::env::temp_dir().join(format!("su-telemetry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let nested = dir.join("logs");

        let writer = MaskingFileWriter::new(&nested.join("test.log"), "/home/michito")
            .expect("writer");

        {
            let mut guard = writer.make_writer();
            guard
                .write_all(b"abriendo /home/michito/fotos/a.png\n")
                .expect("escritura");
            guard.flush().expect("flush");
        }

        let content = std::fs::read_to_string(nested.join("test.log")).expect("lectura");
        assert!(content.contains("<home>/fotos/a.png"));
        assert!(!content.contains("/home/michito"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_path_is_daily() {
        let path = daily_log_path(Path::new("/var/log/su"), "2026-09-15");
        assert!(path.ends_with("su-server-2026-09-15.log"));
    }

    #[test]
    fn installing_the_logger_twice_is_not_an_startup_warning() {
        // BUG-08: la segunda llamada a `init` devolvia un error de `try_init`
        // ("ya hay un subscriber global") y el CLI lo imprimia como "aviso: sin
        // registro en archivo", que es un problema de arranque inventado: el
        // registro estaba funcionando.
        //
        // Se hace todo en una sola prueba porque el registro global es del
        // **proceso**: repartirlo en dos pruebas dejaria el orden en manos del
        // planificador. El directorio es propio de esta prueba para no pisar el
        // log real del usuario.
        let dir = std::env::temp_dir().join(format!("su-telemetry-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let first = init(&dir, "/home/alguien", "info").expect("primer arranque");

        // Se registra algo despues de la primera instalacion, con la ruta personal
        // en el mensaje, para comprobar de paso que el enmascarado sigue puesto.
        tracing::info!(ruta = "/home/alguien/fotos/a.png", "primera linea");

        let second = init(&dir, "/home/alguien", "info").expect("segundo arranque");
        assert_eq!(
            first, second,
            "las dos llamadas tienen que hablar del mismo archivo del dia"
        );

        tracing::info!("segunda linea");

        let content = std::fs::read_to_string(&first).expect("log escrito");
        assert!(
            content.contains("primera linea") && content.contains("segunda linea"),
            "las lineas de las dos etapas tienen que acabar en el archivo: {content}"
        );
        assert!(
            !content.contains("/home/alguien"),
            "el directorio personal no puede aparecer en el log: {content}"
        );
        assert!(content.contains("<home>/fotos/a.png"), "cuerpo: {content}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
