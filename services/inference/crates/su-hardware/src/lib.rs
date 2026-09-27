//! # su-hardware
//!
//! Deteccion de hardware y de que execution providers estan realmente
//! disponibles.
//!
//! Principio: **no se asume nada**. Un EP no se declara disponible porque la
//! plataforma lo permita, sino porque sus bibliotecas estan presentes. Un EP
//! marcado como disponible que luego falla al cargar es peor que uno marcado
//! como ausente: el usuario pierde minutos hasta que se rinde.
//!
//! Toda la logica de interpretacion (salida de `nvidia-smi`, identificadores PCI,
//! inventario de bibliotecas) esta separada de la ejecucion de comandos, para
//! poder cubrirla con tests sin GPU.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use su_tiling::ProviderKind;

/// Orden de prioridad de los EPs (ADR-002). NCNN-Vulkan no aparece: no se
/// empaqueta, solo se documenta como ultimo recurso.
pub const PROVIDER_PRIORITY: &[ProviderKind] = &[
    ProviderKind::TensorRt,
    ProviderKind::Cuda,
    ProviderKind::DirectMl,
    ProviderKind::CoreMl,
    ProviderKind::Cpu,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Unknown,
}

impl GpuVendor {
    /// A partir del identificador de fabricante PCI, tal como lo publica
    /// `/sys/class/drm/card*/device/vendor` en Linux.
    pub fn from_pci_vendor_id(id: &str) -> Self {
        let normalized = id.trim().to_ascii_lowercase();
        let normalized = normalized.trim_start_matches("0x");
        match normalized {
            "10de" => Self::Nvidia,
            "1002" | "1022" => Self::Amd,
            "8086" => Self::Intel,
            "106b" => Self::Apple,
            _ => Self::Unknown,
        }
    }

    /// A partir del nombre comercial, que es lo unico que da `nvidia-smi`.
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("nvidia") || lower.contains("geforce") || lower.contains("quadro") || lower.contains("rtx") {
            Self::Nvidia
        } else if lower.contains("amd") || lower.contains("radeon") || lower.contains("ati ") {
            Self::Amd
        } else if lower.contains("intel") || lower.contains("arc ") || lower.contains("iris") || lower.contains("uhd graphics") {
            Self::Intel
        } else if lower.contains("apple") {
            Self::Apple
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub brand: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
}

impl CpuInfo {
    /// Hilos que debe usar ONNX Runtime en CPU.
    ///
    /// Se usan los nucleos **fisicos**, no los logicos: el hyperthreading no
    /// ayuda a las convoluciones, que ya saturan las unidades de coma flotante, y
    /// sobre-suscribir hilos empeora el rendimiento medido (ADR-014).
    pub fn inference_threads(&self) -> usize {
        self.physical_cores.max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAvailability {
    pub kind: ProviderKind,
    pub available: bool,
    /// Motivo por el que no esta disponible, o por el que se ha descartado.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub cpu: CpuInfo,
    pub gpus: Vec<GpuInfo>,
    pub ram_total_mb: u64,
    pub providers: Vec<ProviderAvailability>,
    /// El primer EP disponible segun prioridad. Siempre existe: CPU.
    pub recommended: ProviderKind,
    /// Version de ONNX Runtime cargada, si se ha podido cargar.
    pub ort_version: Option<String>,
}

impl Capabilities {
    pub fn provider(&self, kind: ProviderKind) -> Option<&ProviderAvailability> {
        self.providers.iter().find(|entry| entry.kind == kind)
    }

    pub fn is_available(&self, kind: ProviderKind) -> bool {
        self.provider(kind).is_some_and(|entry| entry.available)
    }

    /// VRAM libre de la GPU con mas memoria disponible.
    pub fn best_vram_free_mb(&self) -> u64 {
        self.gpus.iter().map(|gpu| gpu.vram_free_mb).max().unwrap_or(0)
    }
}

/// Interpreta la salida de
/// `nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader,nounits`.
///
/// Es texto ajeno: se parsea con tolerancia y se descartan las lineas que no
/// encajan en lugar de fallar entero. Una GPU mal leida no puede impedir usar las
/// demas.
pub fn parse_nvidia_smi_csv(output: &str) -> Vec<GpuInfo> {
    let mut gpus = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() < 3 {
            continue;
        }

        let Some(name) = fields.first().filter(|value| !value.is_empty()) else {
            continue;
        };

        // Sin memoria legible la GPU no sirve para planificar el tiling.
        let (Ok(total), Ok(free)) = (
            fields[1].parse::<u64>(),
            fields[2].parse::<u64>(),
        ) else {
            continue;
        };

        gpus.push(GpuInfo {
            name: (*name).to_string(),
            vendor: GpuVendor::from_name(name),
            vram_total_mb: total,
            vram_free_mb: free,
            driver_version: fields.get(3).map(|value| (*value).to_string()),
        });
    }

    gpus
}

/// Inventario de bibliotecas de execution providers presentes en un directorio.
///
/// ONNX Runtime con `load-dynamic` carga las bibliotecas por nombre desde el
/// directorio del ejecutable. Si el archivo no esta, el EP no existe, por mucho
/// que la GPU sea compatible.
pub fn detect_provider_libraries(dir: &Path) -> Vec<ProviderKind> {
    let mut found = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with("onnxruntime") {
            continue;
        }

        if name.contains("tensorrt") {
            push_unique(&mut found, ProviderKind::TensorRt);
        } else if name.contains("cuda") {
            push_unique(&mut found, ProviderKind::Cuda);
        } else if name.contains("dml") || name.contains("directml") {
            push_unique(&mut found, ProviderKind::DirectMl);
        } else if name.contains("coreml") {
            push_unique(&mut found, ProviderKind::CoreMl);
        }
    }

    found
}

fn push_unique(list: &mut Vec<ProviderKind>, kind: ProviderKind) {
    if !list.contains(&kind) {
        list.push(kind);
    }
}

/// Combina el hardware con las bibliotecas presentes y decide que EPs se pueden
/// usar de verdad, en orden de prioridad.
pub fn resolve_providers(gpus: &[GpuInfo], libraries: &[ProviderKind]) -> Vec<ProviderAvailability> {
    let has_nvidia = gpus.iter().any(|gpu| gpu.vendor == GpuVendor::Nvidia);
    let has_any_gpu = !gpus.is_empty();

    PROVIDER_PRIORITY
        .iter()
        .map(|kind| {
            let (available, reason) = match kind {
                ProviderKind::TensorRt => {
                    if !libraries.contains(kind) {
                        (false, Some("falta onnxruntime_providers_tensorrt".to_string()))
                    } else if !has_nvidia {
                        (false, Some("se requiere una GPU NVIDIA".to_string()))
                    } else {
                        (true, None)
                    }
                }
                ProviderKind::Cuda => {
                    if !libraries.contains(kind) {
                        (false, Some("falta onnxruntime_providers_cuda".to_string()))
                    } else if !has_nvidia {
                        (false, Some("se requiere una GPU NVIDIA".to_string()))
                    } else {
                        (true, None)
                    }
                }
                ProviderKind::DirectMl => {
                    if !libraries.contains(kind) {
                        (false, Some("falta onnxruntime_providers_dml".to_string()))
                    } else if !has_any_gpu {
                        (false, Some("no se detecto ninguna GPU".to_string()))
                    } else {
                        (true, None)
                    }
                }
                ProviderKind::CoreMl => {
                    if !libraries.contains(kind) {
                        (false, Some("falta el proveedor CoreML".to_string()))
                    } else {
                        (true, None)
                    }
                }
                ProviderKind::Cpu => (true, None),
            };

            ProviderAvailability {
                kind: *kind,
                available,
                reason,
            }
        })
        .collect()
}

/// Primer EP disponible segun prioridad. CPU es el suelo: siempre lo esta.
pub fn recommend_provider(providers: &[ProviderAvailability]) -> ProviderKind {
    PROVIDER_PRIORITY
        .iter()
        .copied()
        .find(|kind| {
            providers
                .iter()
                .any(|entry| entry.kind == *kind && entry.available)
        })
        .unwrap_or(ProviderKind::Cpu)
}

/// Sondea el hardware del equipo.
pub fn probe(executable_dir: &Path) -> Capabilities {
    let gpus = probe_gpus();
    let libraries = detect_provider_libraries(executable_dir);
    let providers = resolve_providers(&gpus, &libraries);

    Capabilities {
        cpu: probe_cpu(),
        ram_total_mb: probe_ram_mb(),
        recommended: recommend_provider(&providers),
        gpus,
        providers,
        ort_version: None,
    }
}

/// Nucleos fisicos a partir de `/proc/cpuinfo`.
///
/// Cuenta pares distintos `(physical id, core id)`, que es lo que distingue un
/// nucleo fisico de un hilo de hyperthreading. En arquitecturas donde el archivo
/// no publica esos campos (ARM, por ejemplo) devuelve `None` y el llamador cae a
/// los nucleos logicos.
pub fn parse_cpuinfo_physical_cores(text: &str) -> Option<usize> {
    let mut pairs = std::collections::HashSet::new();
    let mut physical: Option<String> = None;
    let mut core: Option<String> = None;

    let mut flush = |physical: &mut Option<String>, core: &mut Option<String>| {
        if let (Some(p), Some(c)) = (physical.take(), core.take()) {
            pairs.insert((p, c));
        }
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut physical, &mut core);
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "physical id" => physical = Some(value.trim().to_string()),
            "core id" => core = Some(value.trim().to_string()),
            _ => {}
        }
    }
    flush(&mut physical, &mut core);

    if pairs.is_empty() {
        None
    } else {
        Some(pairs.len())
    }
}

/// Modelo de CPU a partir de `/proc/cpuinfo`.
pub fn parse_cpuinfo_brand(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        // x86 publica "model name"; ARM suele publicar "Hardware" o "Model".
        if matches!(key.trim(), "model name" | "Hardware" | "Model") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn read_cpuinfo() -> Option<String> {
    std::fs::read_to_string("/proc/cpuinfo").ok()
}

#[cfg(not(target_os = "linux"))]
fn read_cpuinfo() -> Option<String> {
    None
}

fn probe_cpu() -> CpuInfo {
    // `available_parallelism` es de la biblioteca estandar: no hace falta un crate
    // externo para contar hilos.
    let logical = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);

    match read_cpuinfo() {
        Some(text) => {
            let physical = parse_cpuinfo_physical_cores(&text).unwrap_or(logical);
            let brand = parse_cpuinfo_brand(&text)
                .unwrap_or_else(|| std::env::consts::ARCH.to_string());
            CpuInfo {
                brand,
                physical_cores: physical.max(1),
                logical_cores: logical,
            }
        }
        None => CpuInfo {
            brand: std::env::consts::ARCH.to_string(),
            physical_cores: logical,
            logical_cores: logical,
        },
    }
}

/// Memoria RAM total en MiB.
///
/// En Linux se lee `/proc/meminfo`, que es exacto y no arrastra dependencias. El
/// dato es informativo (para avisar de lotes que no caben en memoria), asi que en
/// las demas plataformas se devuelve 0 en lugar de anadir un crate pesado solo
/// para esto. Se completara al empaquetar para Windows y macOS (Fase 5).
pub fn parse_meminfo_total_mb(text: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kib = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(kib / 1024);
        }
    }
    None
}

fn probe_ram_mb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            if let Some(total) = parse_meminfo_total_mb(&text) {
                return total;
            }
        }
    }
    0
}

/// GPUs NVIDIA via `nvidia-smi`. Si el binario no existe o falla, devuelve una
/// lista vacia: la ausencia de `nvidia-smi` no es un error, es informacion.
fn probe_gpus() -> Vec<GpuInfo> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .output();

    match output {
        Ok(result) if result.status.success() => {
            parse_nvidia_smi_csv(&String::from_utf8_lossy(&result.stdout))
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Directorio de trabajo para los tests que necesitan tocar el disco.
    ///
    /// Se construye con la biblioteca estandar en lugar de anadir `tempfile`: el
    /// unico uso es crear cuatro archivos vacios, y `tempfile` arrastra
    /// `getrandom` y, en Windows, `windows-sys`.
    fn scratch_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("su-hardware-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("directorio temporal");
        dir
    }

    const SAMPLE: &str = "\
NVIDIA GeForce RTX 3060, 12288, 11000, 535.54
NVIDIA GeForce RTX 4090, 24564, 24000, 550.54
";

    #[test]
    fn nvidia_smi_output_is_parsed() {
        let gpus = parse_nvidia_smi_csv(SAMPLE);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 3060");
        assert_eq!(gpus[0].vram_total_mb, 12288);
        assert_eq!(gpus[0].vram_free_mb, 11000);
        assert_eq!(gpus[0].driver_version.as_deref(), Some("535.54"));
        assert_eq!(gpus[0].vendor, GpuVendor::Nvidia);
    }

    #[test]
    fn malformed_lines_are_skipped_without_losing_the_good_ones() {
        let messy = "\
NVIDIA GeForce RTX 3060, 12288, 11000, 535.54

esto no es una gpu
GPU rara, no-es-un-numero, 500
NVIDIA GeForce RTX 4090, 24564, 24000, 550.54
";
        let gpus = parse_nvidia_smi_csv(messy);
        assert_eq!(gpus.len(), 2, "{gpus:?}");
        assert!(gpus.iter().all(|gpu| gpu.vram_total_mb > 0));
    }

    #[test]
    fn empty_output_yields_no_gpus() {
        assert!(parse_nvidia_smi_csv("").is_empty());
        assert!(parse_nvidia_smi_csv("\n\n").is_empty());
    }

    #[test]
    fn driver_version_is_optional() {
        let gpus = parse_nvidia_smi_csv("NVIDIA GeForce GTX 1080, 8192, 8000");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].driver_version, None);
    }

    #[test]
    fn pci_vendor_ids_are_recognised() {
        assert_eq!(GpuVendor::from_pci_vendor_id("0x10de"), GpuVendor::Nvidia);
        assert_eq!(GpuVendor::from_pci_vendor_id("10DE"), GpuVendor::Nvidia);
        assert_eq!(GpuVendor::from_pci_vendor_id("0x1002"), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_pci_vendor_id("0x8086"), GpuVendor::Intel);
        assert_eq!(GpuVendor::from_pci_vendor_id("0x106b"), GpuVendor::Apple);
        assert_eq!(GpuVendor::from_pci_vendor_id("0xffff"), GpuVendor::Unknown);
    }

    #[test]
    fn commercial_names_are_recognised() {
        assert_eq!(GpuVendor::from_name("AMD Radeon RX 7900 XTX"), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_name("Intel Arc A770"), GpuVendor::Intel);
        assert_eq!(GpuVendor::from_name("Apple M3 Max"), GpuVendor::Apple);
        assert_eq!(GpuVendor::from_name("Matrox Mystique"), GpuVendor::Unknown);
    }

    #[test]
    fn provider_libraries_are_discovered() {
        let dir = scratch_dir("libs");
        for name in [
            "onnxruntime.so",
            "onnxruntime_providers_shared.so",
            "onnxruntime_providers_cuda.so",
            "onnxruntime_providers_tensorrt.so",
        ] {
            std::fs::write(dir.join(name), b"").expect("archivo");
        }

        let found = detect_provider_libraries(&dir);
        assert!(found.contains(&ProviderKind::Cuda));
        assert!(found.contains(&ProviderKind::TensorRt));
        assert!(!found.contains(&ProviderKind::DirectMl));
        assert!(!found.contains(&ProviderKind::CoreMl));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let found = detect_provider_libraries(Path::new("/ruta/que/no/existe"));
        assert!(found.is_empty());
    }

    #[test]
    fn cpu_is_always_available() {
        let providers = resolve_providers(&[], &[]);
        assert!(providers.iter().any(|entry| entry.kind == ProviderKind::Cpu && entry.available));
        assert_eq!(recommend_provider(&providers), ProviderKind::Cpu);
    }

    #[test]
    fn tensorrt_without_an_nvidia_gpu_is_rejected_with_a_reason() {
        let amd = vec![GpuInfo {
            name: "AMD Radeon RX 7900 XTX".to_string(),
            vendor: GpuVendor::Amd,
            vram_total_mb: 24576,
            vram_free_mb: 24000,
            driver_version: None,
        }];

        let providers = resolve_providers(&amd, &[ProviderKind::TensorRt, ProviderKind::Cuda]);
        let trt = providers
            .iter()
            .find(|entry| entry.kind == ProviderKind::TensorRt)
            .expect("entrada de TensorRT");

        assert!(!trt.available);
        assert!(trt.reason.is_some(), "debe explicar por que no se puede usar");
        // DirectML no tiene bibliotecas, asi que se cae a CPU.
        assert_eq!(recommend_provider(&providers), ProviderKind::Cpu);
    }

    #[test]
    fn the_best_available_provider_wins_by_priority() {
        let nvidia = vec![GpuInfo {
            name: "NVIDIA GeForce RTX 4090".to_string(),
            vendor: GpuVendor::Nvidia,
            vram_total_mb: 24564,
            vram_free_mb: 24000,
            driver_version: None,
        }];

        let with_trt = resolve_providers(
            &nvidia,
            &[ProviderKind::Cuda, ProviderKind::TensorRt],
        );
        assert_eq!(recommend_provider(&with_trt), ProviderKind::TensorRt);

        // Sin las bibliotecas de TensorRT, gana CUDA.
        let only_cuda = resolve_providers(&nvidia, &[ProviderKind::Cuda]);
        assert_eq!(recommend_provider(&only_cuda), ProviderKind::Cuda);
    }

    #[test]
    fn cpu_inference_uses_physical_cores() {
        let cpu = CpuInfo {
            brand: "test".to_string(),
            physical_cores: 8,
            logical_cores: 16,
        };
        assert_eq!(cpu.inference_threads(), 8);

        // Un equipo donde no se pudieron leer los nucleos fisicos no debe dar 0.
        let unknown = CpuInfo {
            brand: "test".to_string(),
            physical_cores: 0,
            logical_cores: 4,
        };
        assert_eq!(unknown.inference_threads(), 1);
    }

    #[test]
    fn probing_this_machine_always_yields_cpu() {
        let capabilities = probe(Path::new("."));
        assert!(capabilities.cpu.logical_cores >= 1);
        assert!(capabilities.is_available(ProviderKind::Cpu));
    }

    #[test]
    fn meminfo_is_parsed() {
        let sample = "\
MemTotal:       32768420 kB
MemFree:         1234567 kB
MemAvailable:   23456789 kB
";
        assert_eq!(parse_meminfo_total_mb(sample), Some(32000));
    }

    #[test]
    fn meminfo_without_the_field_yields_none() {
        assert_eq!(parse_meminfo_total_mb("MemFree: 100 kB\n"), None);
        assert_eq!(parse_meminfo_total_mb(""), None);
        // Un valor no numerico no debe provocar un panic.
        assert_eq!(parse_meminfo_total_mb("MemTotal: mucho kB\n"), None);
    }

    const CPUINFO_X86: &str = "\
processor\t: 0
vendor_id\t: GenuineIntel
model name\t: Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz
physical id\t: 0
core id\t\t: 0

processor\t: 1
model name\t: Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz
physical id\t: 0
core id\t\t: 1

processor\t: 2
model name\t: Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz
physical id\t: 0
core id\t\t: 0

processor\t: 3
model name\t: Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz
physical id\t: 0
core id\t\t: 1
";

    #[test]
    fn physical_cores_are_counted_from_cpuinfo() {
        // Cuatro procesadores logicos, pero solo dos pares (physical id, core id)
        // distintos: hyperthreading.
        assert_eq!(parse_cpuinfo_physical_cores(CPUINFO_X86), Some(2));
    }

    #[test]
    fn hyperthreading_does_not_inflate_the_physical_count() {
        let logical = CPUINFO_X86.matches("processor\t:").count();
        assert_eq!(logical, 4);
        assert_eq!(parse_cpuinfo_physical_cores(CPUINFO_X86), Some(2));
    }

    #[test]
    fn a_single_socket_without_core_ids_yields_none() {
        // ARM no publica "physical id"/"core id": debe caer al fallback.
        let arm = "processor\t: 0\nBogoMIPS\t: 38.40\n\nprocessor\t: 1\nBogoMIPS\t: 38.40\n";
        assert_eq!(parse_cpuinfo_physical_cores(arm), None);
    }

    #[test]
    fn cpu_brand_is_read() {
        assert_eq!(
            parse_cpuinfo_brand(CPUINFO_X86).as_deref(),
            Some("Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz")
        );
        assert_eq!(
            parse_cpuinfo_brand("Hardware\t: BCM2835\n").as_deref(),
            Some("BCM2835")
        );
        assert_eq!(parse_cpuinfo_brand("BogoMIPS\t: 38.40\n"), None);
    }

    #[test]
    fn best_vram_is_zero_when_there_are_no_gpus() {
        let capabilities = Capabilities {
            cpu: CpuInfo {
                brand: "test".to_string(),
                physical_cores: 4,
                logical_cores: 8,
            },
            gpus: Vec::new(),
            ram_total_mb: 8192,
            providers: resolve_providers(&[], &[]),
            recommended: ProviderKind::Cpu,
            ort_version: None,
        };
        assert_eq!(capabilities.best_vram_free_mb(), 0);
    }
}
