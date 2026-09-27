# 02 · Arquitectura

> Documento técnico de referencia de SuperUpscaly. Define componentes, contratos, algoritmos y estructura de carpetas.

---

## 1. Vista de componentes

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         ELECTRON — MAIN PROCESS (Node.js)                    │
│                                                                              │
│  ┌──────────────┐  ┌────────────────┐  ┌───────────────┐  ┌───────────────┐  │
│  │ WindowManager│  │ IpcRouter      │  │ PathGuard     │  │ SettingsStore │  │
│  │  (ventanas)  │  │ (handlers      │  │ (validación,  │  │ (electron-    │  │
│  │              │  │  tipados+zod)  │  │  canonicaliz.)│  │  store)       │  │
│  └──────────────┘  └───────┬────────┘  └───────────────┘  └───────────────┘  │
│                            │                                                 │
│  ┌─────────────────────────▼──────────────────────────────────────────────┐  │
│  │ SidecarSupervisor                                                      │  │
│  │  · selección de binario por plataforma/arch                            │  │
│  │  · spawn con token + puerto efímero + portfile                         │  │
│  │  · health-check cada 2 s, reinicio con backoff exponencial (1s→30s)    │  │
│  │  · cierre limpio (POST /v1/shutdown → SIGTERM → SIGKILL)               │  │
│  └─────────────────────────┬──────────────────────────────────────────────┘  │
│                            │                                                 │
│  ┌─────────────────────────▼──────────────────────────────────────────────┐  │
│  │ SidecarClient (HTTP + WS)  →  reemite eventos al renderer por IPC      │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────┬─────────────────────────────────────────────┘
                                 │  contextBridge (preload, sandbox: true)
┌────────────────────────────────▼─────────────────────────────────────────────┐
│                     RENDERER — NEXT.JS (static export)                       │
│  React 19 · TailwindCSS · Jotai · i18next                                    │
│  Componentes: Sidebar · DropZone · ModeSelector · ScaleSelector ·            │
│               OutputFolderPicker · UpscalyButton · AdvancedSettings ·        │
│               BatchQueue · ProgressOverlay · Toasts/ErrorPanel               │
│  Estado: atoms Jotai (settings, queue, jobs, capabilities, ui)               │
└──────────────────────────────────────────────────────────────────────────────┘
                                 │  HTTP 127.0.0.1:<efímero>  +  WS /v1/events
                                 │  Authorization: Bearer <token de 32 bytes>
┌────────────────────────────────▼─────────────────────────────────────────────┐
│                  SIDECAR — `su-server` (Rust, tokio + axum)                  │
│                                                                              │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐ │
│  │ su-server  │ │  su-jobs   │ │ su-models  │ │ su-analyze │ │su-hardware │ │
│  │ (API/WS)   │ │ (cola+sqlite)│ │(manifiesto)│ │ (pre-anál.)│ │ (probe)    │ │
│  └─────┬──────┘ └─────┬──────┘ └─────┬──────┘ └─────┬──────┘ └─────┬──────┘ │
│        │              │              │              │              │        │
│  ┌─────▼──────────────▼──────────────▼──────────────▼──────────────▼──────┐ │
│  │                            su-core (dominio)                            │ │
│  │  Job · JobItem · Pipeline · Stage · ImageRef · Analysis · SuError       │ │
│  └─────────────────────────────┬───────────────────────────────────────────┘ │
│                                │                                             │
│  ┌─────────────────────────────▼───────────────────────────────────────────┐ │
│  │ su-inference                                                             │ │
│  │  · SessionPool (1 sesión por modelo+EP+shape, LRU)                       │ │
│  │  · ExecutionProviderRegistry (TRT→CUDA→DML→CoreML→CPU)                   │ │
│  │  · PipelineRunner (DAG lineal, etapas condicionales)                     │ │
│  │  · VramBudget + TileScheduler (su-tiling)                                │ │
│  └─────────────────────────────┬───────────────────────────────────────────┘ │
│                                │                                             │
│  ┌─────────────────────────────▼───────────────────────────────────────────┐ │
│  │ su-imageio   (decode/encode, EXIF, ICC, ZIP/CBZ, escritura atómica)      │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                │                                             │
│  ┌─────────────────────────────▼───────────────────────────────────────────┐ │
│  │ ONNX Runtime (carga dinámica)                                            │ │
│  │  onnxruntime.dll + onnxruntime_providers_{tensorrt,cuda,shared}.dll     │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Principio rector

**Los píxeles no cruzan fronteras de proceso.** El renderer nunca envía ni recibe buffers de imagen; solo rutas de archivo, metadatos y eventos de progreso. El sidecar abre, procesa y escribe en disco. Esto elimina el coste de serialización, evita límites de tamaño de mensaje IPC y hace que el uso de memoria sea predecible.

---

## 2. Contratos y protocolo

### 2.1 Arranque del sidecar

1. Electron genera un **token** de 32 bytes (`crypto.randomBytes(32).toString('hex')`) y una ruta de *portfile* en el directorio de datos de la app.
2. Electron lanza:
   ```
   su-server --host 127.0.0.1 --port 0 --portfile <path> --data-dir <appData> --log-level info
   ```
   con el token en la variable de entorno `SU_TOKEN` (nunca como argumento de línea de comandos: es visible en el listado de procesos).
3. El sidecar abre el socket en un puerto libre elegido por el SO y escribe el portfile de forma atómica:
   ```json
   { "port": 51423, "pid": 12345, "version": "1.0.0", "startedAt": "2026-09-15T17:00:00Z", "protocol": 1 }
   ```
4. Electron hace polling del portfile (máx. 10 s), luego `GET /v1/health`.
5. Si el protocolo mayor no coincide, Electron muestra un error de versión incompatible en lugar de fallar de forma opaca.

### 2.2 Reglas de seguridad del transporte

- Bind exclusivo a `127.0.0.1`. Nunca `0.0.0.0`.
- Todas las rutas excepto `/v1/health` requieren `Authorization: Bearer <token>`.
- El token se compara en tiempo constante (`subtle::ConstantTimeEq`).
- Rechazo de peticiones con `Origin` o `Host` inesperados (protección contra DNS rebinding desde un navegador local).
- `Content-Type: application/json` obligatorio en POST; cuerpo máximo 1 MiB (los payloads son rutas y metadatos, nunca imágenes).
- CORS deshabilitado: no se responde a preflight.
- El servidor se cierra solo cuando Electron lo pide, o si el proceso padre muere (watchdog de pipe: el sidecar lee de stdin y termina si el pipe se cierra).

### 2.3 Endpoints (v1)

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/v1/health` | Liveness. Sin autenticación. Devuelve `{ status, version }` |
| GET | `/v1/capabilities` | Hardware, EPs disponibles, GPUs, VRAM libre/total, núcleos, RAM y el **motor** que corre de verdad (`engine`) |
| GET | `/v1/models` | Modelos del manifiesto + estado local (`installed`, `unverified`, `missing`, `hashMismatch`) y `modelsDir` |
| GET | `/v1/pipelines` | Pipelines cargados, con sus etapas y condiciones. Lo usa la interfaz para dibujar la cadena y para saber **qué modelo descargar** (ADR-026/028) |
| POST | `/v1/jobs` | Crea un trabajo de lote. Responde `201` |
| GET | `/v1/jobs` | Lista de trabajos, del más reciente al más antiguo |
| GET | `/v1/jobs/{id}` | Estado detallado + items |
| POST | `/v1/jobs/{id}/pause` · `/resume` · `/cancel` | Control de ejecución |
| DELETE | `/v1/jobs/{id}` | Cancela el trabajo (equivalente a `/cancel`) |
| GET | `/v1/events` | WebSocket con el flujo de eventos |
| POST | `/v1/shutdown` | Cierre ordenado, pedido por la aplicación |

> Esta tabla es la del **enrutador que existe** (`crates/su-server/src/lib.rs`).
> Instalar y borrar modelos (`POST /v1/models/install`, `DELETE /v1/models/{id}`)
> **no son rutas del sidecar a propósito**: la descarga vive en la aplicación, que
> es quien tiene el proxy del sistema, el directorio del usuario y el progreso en
> la interfaz (ADR-019). El análisis previo tampoco es un endpoint: ocurre dentro
> del trabajo, sobre la imagen ya decodificada, y su resultado viaja en las
> variables que evalúan los pipelines. `Idempotency-Key`, la paginación y el
> benchmark por HTTP son ideas del diseño inicial que **no se implementaron**;
> lamentablemente no se pueden ofrecer como si existieran, así que quedan fuera de
> la tabla. Si hacen falta, el sitio es este documento y no el código.
| GET | `/v1/events` | **WebSocket**. Stream de eventos |
| POST | `/v1/shutdown` | Cierre ordenado |

### 2.4 Formato de eventos (WebSocket)

```jsonc
{ "seq": 1043, "ts": "2026-09-15T17:04:11.212Z", "type": "item.progress",
  "jobId": "01J...", "itemId": "01J...-007",
  "data": { "stage": "upscale", "stageIndex": 2, "stageCount": 4,
            "tilesDone": 34, "tilesTotal": 128, "percent": 0.265,
            "tileSize": 512, "ep": "TensorRT", "vramPeakMb": 3180 } }
```

Tipos de evento: `job.created`, `job.started`, `job.paused`, `job.resumed`, `job.progress`, `job.completed`, `job.failed`, `job.cancelled`, `item.started`, `item.progress`, `item.completed`, `item.failed`, `item.degraded`, `model.download`, `hardware.changed`, `log`, `warning`, `error`.

**Backpressure:** el canal WS tiene capacidad acotada (256 mensajes). Los eventos `item.progress` se **coalescen** (máximo 10/s por item); los eventos de estado (`job.*`, `item.completed`) nunca se descartan. Si el renderer se queda atrás, el sidecar descarta progreso intermedio y emite un evento `job.progress` consolidado.

### 2.5 Contrato TypeScript ↔ Rust

- Rust genera `openapi.json` con `utoipa`.
- `scripts/gen-client.mjs` ejecuta `openapi-typescript` → `packages/shared/src/api.generated.ts`.
- Sobre los tipos generados se construyen esquemas zod en `packages/shared/src/schemas.ts`.
- CI falla si `openapi.json` cambia sin regenerar el cliente (evita desincronización).

---

## 3. Capa de inferencia

### 3.1 Selección de Execution Provider

Orden de prioridad y criterios:

| Prioridad | EP | Plataforma | Criterio de aceptación |
|---|---|---|---|
| 1 | TensorRT | Windows/Linux + NVIDIA | `nvml` detecta GPU con CC ≥ 7.5 y existe `onnxruntime_providers_tensorrt` |
| 2 | CUDA | Windows/Linux + NVIDIA | Existe `onnxruntime_providers_cuda`, driver ≥ mínimo |
| 3 | DirectML | Windows | `DXGI` enumera adaptador con soporte D3D12 |
| 4 | CoreML | macOS | `MLModel` disponible (siempre en macOS 12+) |
| 5 | CPU | Todas | Siempre disponible |
| 6 | NCNN-Vulkan *(opcional)* | Todas | Solo si se instala explícitamente y los 5 anteriores fallan |

Cada EP tiene un **test de humo** en `su-hardware`: se ejecuta un modelo diminuto (una convolución 1×1) y se mide el tiempo. Un EP que falla el test se marca `unavailable` y se pasa al siguiente, registrando el motivo en `capabilities`.

**Precisión:** `fp16` por defecto en GPU (TRT/CUDA/CoreML), `fp32` en CPU y DirectML salvo que el modelo declare `fp16Safe: false` en el manifiesto. La elección es por modelo, no global.

**Caché de motores TensorRT:** directorio `cache/trt/<hash>` donde `hash = blake3(driverVersion + ortVersion + modelSha256 + profileShapes)`. Los profiles de forma se precompilan para el conjunto discreto de tiles (256/384/512/768/1024) más la forma de imagen completa si cabe. La construcción del motor se reporta como etapa visible en la UI (puede tardar minutos la primera vez) — nunca como un cuelgue.

### 3.2 SessionPool

- Clave: `(modelId, ep, deviceId, shapeProfile, precision)`.
- Valor: `ort::Session` + buffers de E/S preasignados + estadísticas de VRAM.
- Política: LRU con límite configurable (por defecto 3 sesiones, o 1 si VRAM libre < 4 GB).
- `unloadBetweenImages` (opción avanzada): libera todas las sesiones tras cada imagen; útil en GPUs con poca VRAM, a costa de ~1–3 s por recarga.
- La liberación real se verifica con `nvml` (VRAM libre recuperada) y se registra; si no se recupera, se emite `warning` y se sugiere reducir el tile.

### 3.3 Motor de pipelines

Un pipeline es una secuencia ordenada de etapas con condición opcional. Formato declarativo en `pipelines.json` (ver [04 · Modelos y pipelines](04-modelos-y-pipelines.md)).

Etapas soportadas:

| `op` | Función |
|---|---|
| `analyze` | Ejecuta el análisis previo y publica variables (`faces`, `noise`, `kind`, `confidence`) |
| `model` | Inferencia con un modelo ONNX; admite `tiling`, `blend`, `blendFrom`, `scaleOut`, `onlyOnFaces`, `overlapBoost`, `kernel` |
| `resize` | Reescalado clásico (`lanczos3`, `mitchell`, `nearest`) con factor o tamaño destino |
| `unsharp` | Enfoque unsharp-mask (`amount`, `radius`, `threshold`, los tres en la escala `0..1`) |
| `denoise_classic` | Reducción de ruido no-ML (bilateral guiado) para imágenes levemente degradadas |
| `compose` | Mezcla de dos ramas (p. ej. cara restaurada sobre la base) con máscara |

Las condiciones se evalúan con una expresión restringida (sin evaluación dinámica de código: se compila a un AST propio y se interpreta). Variables disponibles: `analysis.*`, `prefs.*`, `hardware.*`.

### 3.4 Estrategia de escala (2x / 4x / 8x)

| Escala | Fotos | Dibujo/Anime |
|---|---|---|
| **2x** | Modelo x4 → `resize` lanczos3 a 0.5 | Modelo 2x nativo (`2x-AnimeSharpV3`) si existe; si no, x4 + lanczos3 |
| **4x** | Modelo x4 directo | Modelo x4 anime directo |
| **8x** | x4 → x4 (doble pasada con solape) | x4 anime → x4 anime |

Notas:
- El `resize` a 0.5 se hace en **coma flotante de 32 bits por canal** (`image::imageops` sobre `Rgb32F`/`Rgba32F`) para no perder información en la reducción. Hasta ADR-025 este paso cuantizaba a 8 bits por canal.
- La doble pasada de 8x usa solape mayor (tile/8) para evitar costuras acumuladas.
- Si la imagen de entrada ya es ≥ 4096 px en el lado mayor y se pide 8x, la UI avisa del tiempo estimado y del espacio en disco requerido.

---

## 4. Tiling adaptativo y gestión de VRAM

### 4.1 Presupuesto de VRAM

```
presupuesto = VRAM_libre × factor_seguridad − overhead_contexto − overhead_pesos
```

| Término | Valor / origen |
|---|---|
| `factor_seguridad` | 0.70 (configurable; 0.55 en GPUs con ≤4 GB) |
| `overhead_contexto` | Medido empíricamente: ~250 MB TRT, ~180 MB CUDA, ~120 MB DML, ~60 MB CoreML |
| `overhead_pesos` | `modelBytes` del manifiesto (pesos + ~15 % de workspace) |
| `VRAM_libre` | `nvml` (NVIDIA), `DXGI QueryVideoMemoryInfo` (Windows), `MTLDevice.recommendedMaxWorkingSetSize` (macOS), ignorado en CPU |

### 4.2 Calibración por modelo

Cada modelo declara en el manifiesto un `vramPerMegapixel` estimado. La primera vez que se ejecuta, el sistema mide el pico real (muestreo de `nvml` a 20 Hz durante la inferencia) y guarda el valor en `cache/calibration.json`:

```json
{ "4x-ultrasharp|TensorRT|0|512": { "vramPerMegapixel": 412.5, "msPerMegapixel": 380.2, "samples": 7 } }
```

Con eso, la elección de tile deja de ser heurística y pasa a ser predictiva.

### 4.3 Algoritmo de selección de tile

```
1. Si la imagen completa cabe en el presupuesto → tile = "full" (sin tiling, máxima calidad)
2. Si no:
   para cada candidato t en [1024, 768, 512, 384, 256, 192] descendente:
       vram_predicha = t² / 1e6 × vramPerMegapixel + overhead_pesos
       si vram_predicha ≤ presupuesto → elegir t y salir
3. Si ningún candidato cabe → t = 128; si tampoco → fallback a CPU
4. overlap = clamp(t / 16, 16, 64)  (en píxeles de entrada)
5. Pad de cada tile hasta múltiplo de 32 para que TRT/DML usen formas alineadas
```

### 4.4 Composición sin costuras

- Cada tile se infiere con `overlap` píxeles extra por lado.
- Al pegar, la zona de solape se mezcla con una **ventana coseno elevada** (`cos²`), que es separable y por tanto O(n) en lugar de O(n²).
- Los píxeles fuera de la imagen original se rellenan con **reflejo** (`reflect`), no con negro: el relleno negro genera halos en los bordes (defecto clásico de otras implementaciones).
- Los tiles de borde se recortan a la extensión real de la imagen tras la inferencia.

### 4.5 Degradación progresiva (anti-OOM)

```
intento 0: tile = seleccionado
  └─ OOM / EP error
intento 1: tile = tile / 2          (mín. 128)
intento 2: tile = tile / 2
intento 3: liberar sesiones + unloadBetweenImages = true
intento 4: EP = CPU (intra_op = núcleos físicos)  → item marcado "degraded"
  └─ si también falla → item.failed con código SU-E130 y detalle del EP y la forma
```

Cada degradación se reporta en la UI (icono ámbar en el item + entrada en el log) y se resume al final del lote. **Nunca se falla en silencio.**

### 4.6 Validación de salida

Antes de escribir el archivo final:

1. Dimensiones exactas esperadas (`round(w·s)` × `round(h·s)`).
2. El buffer no es uniforme: `std_dev(luma) > 1.0` y no es 100 % negro ni 100 % blanco.
3. No contiene `NaN`/`Inf` (posible con fp16 en modelos mal convertidos).
4. Comparación de rango dinámico contra la entrada: si el pico de luminancia es < 10 % del de la entrada, se marca sospechoso.

Si la validación falla → `SU-E141 OutputValidationFailed`, el archivo **no se escribe** y se conserva el tile crudo en el directorio de trabajo para diagnóstico.

### 4.7 Paralelismo

| Tipo de worker | Cantidad por defecto | Notas |
|---|---|---|
| Inferencia GPU | 1 por GPU | 2 sesiones concurrentes en la misma GPU casi nunca mejora el throughput y duplica la VRAM |
| Inferencia CPU | 1 sesión con `intra_op_num_threads = núcleos físicos` | Varias sesiones en paralelo compiten por ancho de banda de memoria; medido y descartado |
| Decodificación | `max(2, núcleos_físicos / 2)` | Canal acotado (capacidad 2) |
| Codificación | `max(2, núcleos_físicos / 2)` | Canal acotado (capacidad 2) |
| Multi-GPU | 1 worker por GPU, reparto *work-stealing* sobre los items | Los tiles de una misma imagen van siempre a la misma GPU (evita transferencias) |

Pipeline asíncrono por item:

```
[decode i+1] ──┐
               ├──► [infer i] ──► [encode i-1]
[decode i+2] ──┘
```

Los canales acotados propagan la presión de memoria hacia atrás: si la GPU va lenta, la decodificación se frena sola. No hay cola ilimitada de imágenes decodificadas en RAM.

---

## 5. E/S de imagen

| Función | Implementación |
|---|---|
| Decodificación | crate `image` (PNG, JPEG, WEBP, BMP, TIFF) + `zune-jpeg` para JPEG (más rápido) |
| Codificación | `image` + `png` con `Compression::Fast` por defecto (configurable a `Best`) |
| Reescalado clásico | `image::imageops` en `f32` (SIMD con `fast_image_resize`, pendiente) |
| EXIF | `kamadak-exif`; **la orientación se aplica siempre antes de inferir** |
| ICC | `lcms2`; el perfil se preserva en la salida cuando el formato lo permite |
| Alpha | La silueta se escala con **el mismo modelo y la misma malla de tiles que el color**, dentro del pipeline (ADR-029): un interpolador aparte dejaba una rampa de 4–7 px que se ve como un halo alrededor del contorno. Los píxeles totalmente transparentes toman el color del visible más cercano antes de inferir (ADR-027) |
| ZIP/CBZ | `zip` crate; se extrae a `cache/jobs/<id>/extract/`, se procesa y se reempaqueta en ZIP si el usuario lo pide |
| Escritura | **Atómica**: escribir en `<destino>.su-tmp` → `fsync` → `rename`. Un fallo a mitad nunca deja un archivo parcial |

Formatos de salida: `png` (defecto, sin pérdida), `jpg` (calidad configurable, 95 por defecto), `webp` (sin pérdida o calidad 90).

Metadatos: opción `preserveMetadata` (por defecto activada para EXIF básico: fecha, orientación ya aplicada, y `Software: SuperUpscaly <version>`).

---

## 6. Especificación de UI

### 6.1 Layout (paridad con Upscayl)

```
┌──────────────┬───────────────────────────────────────────────────────────────┐
│              │                                                               │
│   SIDEBAR    │                       ÁREA PRINCIPAL                          │
│   (280 px)   │                                                               │
│              │   ┌───────────────────────────────────────────────────────┐   │
│  [ Logo ]    │   │                                                       │   │
│  SuperUpscaly│   │              ZONA DE ARRASTRAR Y SOLTAR               │   │
│              │   │        "Arrastra y suelta tus imágenes aquí"          │   │
│  MODO        │   │                                                       │   │
│  ( ) Fotos   │   │        [ Seleccionar imagen(es) ]                     │   │
│  (•) Dibujo  │   │                                                       │   │
│      /Anime  │   └───────────────────────────────────────────────────────┘   │
│              │                                                               │
│  ESCALA      │   Carpeta de salida:  ~/Pictures/Upscaled   [ Cambiar ]        │
│  [2x][4x][8x]│                                                               │
│              │   ┌───────────────────────────────────────────────────────┐   │
│  ► Avanzado  │   │                    [   UPSCALY   ]                    │   │
│              │   └───────────────────────────────────────────────────────┘   │
│  v0.1.0      │                                                               │
└──────────────┴───────────────────────────────────────────────────────────────┘
```

### 6.2 Paleta morada

| Token | Hex | Uso |
|---|---|---|
| `bg-base` | `#1E1B2E` | Fondo principal de la ventana |
| `bg-surface` | `#2D2640` | Tarjetas, sidebar, zona de drop |
| `bg-surface-2` | `#382F52` | Elementos elevados, hover de superficie |
| `accent` | `#8B5CF6` | Botón principal, selección activa, foco |
| `accent-hover` | `#A78BFA` | Hover del acento |
| `accent-muted` | `#6D28D9` | Estados pulsados, barras de progreso |
| `text-primary` | `#F3F4F6` | Texto principal |
| `text-secondary` | `#C4B5FD` | Texto secundario, etiquetas |
| `border` | `#4C1D95` | Bordes, separadores |
| `success` | `#34D399` | Item completado |
| `warning` | `#FBBF24` | Item degradado / aviso |
| `error` | `#F87171` | Item fallido, mensajes de error |

Se implementan como variables CSS en `globals.css` y se exponen a Tailwind vía `packages/config/tailwind-preset.ts`, de modo que ningún componente use hex literales.

### 6.3 Componentes y estados

| Componente | Estados que debe cubrir |
|---|---|
| `DropZone` | vacío · arrastrando encima · con archivos · error de ruta · procesando |
| `ModeSelector` | Fotos · Dibujo/Anime · recomendación del análisis (badge "Recomendado") |
| `ScaleSelector` | 2x · 4x · 8x · deshabilitado si el modelo elegido no lo soporta |
| `UpscalyButton` | activo · deshabilitado (sin imágenes) · procesando (barra + %) · cancelar |
| `OutputFolderPicker` | ruta por defecto (`~/Pictures/Upscaled`) · personalizada · no escribible |
| `AdvancedSettings` | colapsado por defecto; tile (Auto/256/384/512/768/1024), dispositivo (Auto/GPU/CPU), cadena de modelos (Auto/Manual), restauración facial (solo Fotos) |
| `BatchQueue` | lista con miniatura, nombre, tamaño, estado, progreso, tiempo, botón reintentar |
| `ProgressOverlay` | progreso global, ETA, pausa/cancelar, resumen final |
| `ErrorPanel` | código de error, mensaje traducido, detalle técnico expandible, acción sugerida, "Abrir carpeta de logs" |

### 6.4 Reglas de UX

- El botón principal siempre visible; nunca se desplaza fuera de pantalla.
- Todo error tiene: código, mensaje en lenguaje natural y **una acción sugerida**.
- El progreso nunca se muestra como "indeterminado" durante más de 2 s sin explicar qué está ocurriendo (p. ej. "Compilando motor TensorRT…").
- Al terminar un lote, resumen con éxitos/fallos/tiempos y botón "Abrir carpeta de salida".
- Atajos: `Ctrl/Cmd+O` abrir archivos, `Ctrl/Cmd+Shift+O` abrir carpeta, `Enter` iniciar, `Esc` cancelar.
- Accesibilidad: contraste AA mínimo, navegación completa por teclado, `aria-live` en el progreso, sin dependencia exclusiva del color.

### 6.5 Ventana

- Tamaño mínimo 1024×700; por defecto 1280×860.
- Barra de título nativa en Windows/Linux; `titleBarStyle: 'hiddenInset'` en macOS con controles integrados.
- Restauración de tamaño y posición entre sesiones.
- Tema oscuro fijo (la paleta morada es oscura por diseño); no se implementa tema claro.

---

## 7. Análisis pre-upscaling

Ejecutado en `su-analyze`, sobre una versión reducida (lado mayor ≤ 1024 px) para que sea rápido (< 150 ms típico).

| Salida | Método | Uso |
|---|---|---|
| `faces[]` | `YuNet` (ONNX, 337 KB) — **no implementado**: hoy la lista sale vacía | Recortar cada rostro para la restauración facial (ADR-032) |
| `noise` (σ 0–1) | Estimador wavelet de Donoho (puro CPU, ~5 ms por 2 MP) | Decidir si se añade etapa de denoise |
| `kind` | Heurísticas + clasificador `MobileNetV3-Small` (ONNX, ~10 MB) — **no implementado**: hoy es `Unknown` con confianza 0, así que el pipeline nunca contradice al usuario | Confirmar la elección del usuario |
| `resolution`, `aspect`, `hasAlpha` | Decodificación de cabecera | Estimar tiempo y espacio en disco |
| `exif.orientation` | `kamadak-exif` | Rotar antes de inferir |
| `blockiness` | Varianza de la rejilla 8×8 | Detectar JPEG muy comprimido → sugerir denoise |

**Regla de recomendación:** si `kind` contradice la elección del usuario con `confidence > 0.80`, la UI muestra un aviso no bloqueante: *"Esta imagen parece una ilustración. ¿Quieres cambiar al modo Dibujo/Anime?"*. Nunca se cambia el modo automáticamente sin consentimiento.

**Heurísticas de `kind`** (previas al clasificador, para poder funcionar sin el modelo): número de colores únicos tras cuantización, entropía del histograma de matiz, proporción de regiones planas (gradiente < umbral), nitidez de bordes vs ruido, presencia de tramado/screentone.

---

## 8. Cola de trabajos y reanudación

### 8.1 Modelo de datos

```ts
type JobStatus = 'queued' | 'running' | 'paused' | 'completed' | 'partial' | 'failed' | 'cancelled'

interface Job {
  id: string                 // ULID ordenable
  createdAt: string
  status: JobStatus
  mode: 'photo' | 'illustration'
  scale: 2 | 4 | 8
  pipelineId: string
  output: {
    dir: string
    format: 'png' | 'jpg' | 'webp'
    quality: number
    suffix: string           // "_upscaled" por defecto
    preserveMetadata: boolean
    zipOutput: boolean
  }
  options: {
    tileSize: 'auto' | number
    device: 'auto' | `gpu:${number}` | 'cpu'
    concurrency: number      // por defecto 1 por GPU
    unloadBetweenImages: boolean
    faceRestore: 'off' | 'auto' | number   // intensidad 0..1
    denoise: 'off' | 'auto' | 'on'
    sharpen: boolean
    modelOverride?: { upscale?: string; denoise?: string; face?: string }
  }
  progress: { total: number; done: number; failed: number; degraded: number; etaMs?: number }
  items: JobItem[]
}

interface JobItem {
  id: string
  srcPath: string
  outPath?: string
  status: 'pending' | 'running' | 'done' | 'failed' | 'skipped'
  attempt: number
  analysis?: Analysis
  effectivePipeline?: string[]      // ids de etapa realmente ejecutados
  timings?: { decodeMs: number; inferMs: number; encodeMs: number; totalMs: number }
  vramPeakMb?: number
  error?: { code: string; message: string; detail?: string; ep?: string; recoverable: boolean }
  checkpoint?: { stageIndex: number; tileRow: number; tilesDir: string }
}
```

### 8.2 Persistencia

SQLite en `<appData>/jobs.db` (WAL activado). Tablas: `jobs`, `job_items`, `job_events`, `calibration`, `model_state`. Índices por `(job_id, status)` y `(status, priority)`.

### 8.3 Reanudación

| Nivel | Mecanismo |
|---|---|
| Lote | Al arrancar, los jobs en `running` pasan a `paused` con motivo `app-closed`; la UI ofrece reanudar |
| Item | Los items `done` se saltan si el archivo de salida existe y su tamaño > 0 (verificación rápida opcional por hash) |
| Imagen gigante | Los tiles completados se guardan en `cache/jobs/<jobId>/<itemId>/tiles/<stage>/<r>_<c>.png`; al reanudar se saltan los existentes |
| Reintentos | `attempt` con backoff; máximo 3 por item, configurable |

Los temporales se limpian al eliminar el job o al cerrar la app si el job terminó correctamente. Un recolector de basura elimina directorios de trabajo huérfanos con más de 7 días.

### 8.4 Prioridad y concurrencia

- Los jobs tienen `priority` (0–9); dentro de un job, los items se procesan en orden FIFO.
- `concurrency` limita los items simultáneos por GPU (por defecto 1).
- `pause` se implementa como una comprobación cooperativa entre tiles: el trabajo se detiene en un punto seguro (fin de tile), nunca a mitad de una inferencia.

---

## 9. Drag & drop y validación de rutas

### 9.1 El problema real

Desde Electron 32, `File.path` fue eliminado. El patrón correcto es `webUtils.getPathForFile(file)` **en el preload**. Además:

- **Wayland:** el portal XDG puede no exponer rutas reales; el resultado puede ser una ruta vacía o un descriptor `/proc/self/fd/N`.
- **macOS sandbox:** las rutas recibidas pueden no ser legibles sin *security-scoped bookmarks*.

### 9.2 Implementación

```ts
// preload/index.ts
contextBridge.exposeInMainWorld('su', {
  getPathsForFiles: (files: File[]) =>
    files.map((f) => { try { return webUtils.getPathForFile(f) } catch { return '' } }),
  // ...
})
```

Flujo:

1. `onDrop` → `su.getPathsForFiles(e.dataTransfer.files)`.
2. Las rutas vacías o no absolutas se descartan y se cuentan.
3. La UI llama a `window.su.validatePaths(paths)` → main process:
   - `fs.realpath` (resuelve symlinks y normaliza),
   - comprueba que existe y es legible (`fs.access R_OK`),
   - en Windows, normaliza a `\\?\` si la ruta supera 240 caracteres,
   - descarta rutas con bytes NUL, con longitud > 4096, o con componentes `..` inesperados.
4. Si alguna ruta se descarta → toast claro: *"2 archivos no se pudieron leer. Selecciónalos con el botón de archivos."* + botón que abre el diálogo nativo.
5. El sidecar recibe solo rutas ya validadas.

**Prevención de crashes:** el `drop` handler siempre hace `preventDefault()` en `dragover` y `drop`; se ignoran drops sin `dataTransfer.files` (p. ej. texto arrastrado desde el navegador); los drops masivos (> 5000 archivos) se truncan con aviso en lugar de intentar construir el array completo.

### 9.3 Selección por diálogo

`dialog.showOpenDialog` con `properties: ['openFile', 'multiSelections']` o `['openDirectory']`. Las carpetas se expanden en el main process (recursivo, con límite configurable y respetando filtros de extensión) y se reporta el recuento antes de crear el job.

---

## 10. Seguridad

| Vector | Mitigación |
|---|---|
| Ejecución de comandos | **No existe ningún `shell`/`exec`.** El sidecar se lanza con `spawn` y argumentos fijos; nunca se interpola entrada del usuario en una línea de comandos |
| Renderer comprometido | `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`, `webSecurity: true`, `allowRunningInsecureContent: false` |
| Navegación | `will-navigate` bloqueado; `setWindowOpenHandler` → `deny`; `shell.openExternal` solo con lista blanca de esquemas (`https:`) y dominios conocidos |
| CSP | `default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:*; object-src 'none'` |
| API del preload | Superficie mínima, cada método valida sus argumentos con zod antes de cruzar el IPC |
| Rutas | Canonicalización + comprobación de legibilidad + rechazo de NUL/UNC no solicitadas (ver §9.2) |
| Imágenes hacia la ventana | Se sirven por el esquema propio (`app://superupscaly/media?p=…`), que ya cubre `'self'` en la CSP, y **solo** si el proceso principal autorizó esa ruta antes: la interfaz no puede pedir un archivo que no le hayan dado (ADR-035) |
| Sidecar | Bind a loopback, token de 32 bytes, comparación en tiempo constante, comprobación de `Host`/`Origin` |
| Modelos descargados | Verificación de `sha256` **obligatoria** contra el manifiesto; firma del manifiesto (Ed25519) con clave pública embebida; nunca se ejecuta un modelo sin verificar |
| Deserialización | Ningún formato de modelo ejecutable arbitrario; solo ONNX y datos (JSON/PNG) |
| Telemetría | Opt-in explícito, apagada por defecto, sin rutas ni imágenes (ver §12) |
| Actualizaciones | El manifiesto de modelos se firma; el actualizador de la app usa firmas de plataforma (Authenticode / notarización / GPG en AppImage) |

---

## 11. Estructura de carpetas del repositorio

```
superupscaly/
├─ package.json                       # raiz npm workspaces + scripts
├─ tsconfig.base.json
├─ .editorconfig  .gitignore  .nvmrc
│
├─ apps/
│  ├─ desktop/                        # ELECTRON
│  │  ├─ src/main/
│  │  │  ├─ index.ts                  # bootstrap, single-instance lock, menu
│  │  │  ├─ windows.ts                # creacion, estado de ventana, navegacion
│  │  │  ├─ protocol.ts               # esquema app:// + CSP + servicio de imagenes
│  │  │  ├─ files/pathguard.ts        # validacion, canonicalizacion, expansion
│  │  │  ├─ media/registry.ts         # rutas que la ventana tiene permitido mostrar
│  │  │  ├─ ipc/index.ts              # handlers tipados + comprobacion de emisor
│  │  │  ├─ store/settings.ts         # ajustes persistentes (escritura atomica)
│  │  │  ├─ logging/logger.ts         # logs JSON con enmascarado de rutas
│  │  │  └─ sidecar/                  # (Fase 2) supervisor + cliente HTTP/WS
│  │  ├─ src/preload/index.ts         # contextBridge (incl. getPathForFile)
│  │  ├─ resources/
│  │  │  ├─ bin/{win32-x64,darwin-arm64,darwin-x64,linux-x64}/…   # (Fase 2)
│  │  │  └─ icons/                                                # (Fase 5)
│  │  ├─ electron-builder.yml                                     # (Fase 5)
│  │  └─ package.json
│  │
│  └─ renderer/                       # NEXT.JS (output: 'export')
│     ├─ src/app/{layout.tsx,page.tsx,globals.css}
│     ├─ src/components/
│     │  ├─ Sidebar.tsx · ModeSelector.tsx · ScaleSelector.tsx
│     │  ├─ AdvancedSettings.tsx · LanguageToggle.tsx
│     │  ├─ DropZone.tsx · QueueList.tsx · OutputFolderPicker.tsx
│     │  └─ ActionBar.tsx · SummaryDialog.tsx · CompareView.tsx · Toaster.tsx
│     ├─ src/state/
│     │  ├─ atoms/{settings,queue,app,run,ui,compare}.ts
│     │  └─ useRun.ts
│     ├─ src/lib/{bridge,format,ingest,mockRunner,pipelinePlan}.ts
│     ├─ src/types/global.d.ts
│     ├─ next.config.mjs · postcss.config.mjs · tsconfig.json
│     └─ package.json
│
├─ packages/
│  ├─ shared/                         # contratos compartidos
│  │  ├─ src/{types,api,theme,error-codes,guards,media}.ts
│  │  ├─ src/i18n/{es,en,index}.ts
│  │  └─ package.json
│  └─ ui/                             # primitivas reutilizables
│     ├─ src/{Button,Card,Progress,SegmentedControl,Switch,Field,Tooltip,cx}.tsx
│     └─ package.json
│
├─ services/inference/                # WORKSPACE CARGO  (Fase 2)
│  ├─ Cargo.toml                      # [workspace] members
│  └─ crates/
│     ├─ su-server/                   # axum: API HTTP + WS + auth + lifecycle
│     ├─ su-core/                     # dominio, errores, tipos, pipelines
│     ├─ su-inference/                # SessionPool, EP registry, PipelineRunner
│     ├─ su-tiling/                   # seleccion de tile, solape, blending, padding
│     ├─ su-analyze/                  # rostros, ruido, tipo de contenido
│     ├─ su-models/                   # manifiesto, descarga, verificacion, cache
│     ├─ su-hardware/                 # NVML / DXGI / Metal / CPU probe
│     ├─ su-imageio/                  # decode/encode, EXIF, ICC, ZIP, escritura atomica
│     ├─ su-jobs/                     # cola, SQLite, reanudacion, checkpointing
│     ├─ su-telemetry/                # tracing, logs rotativos, metricas
│     └─ su-cli/                      # CLI headless (tests, CI, power users)
│
├─ models/                            # (Fase 4)
│  ├─ manifest.json                   # catalogo (semilla, se actualiza remotamente)
│  ├─ manifest.sig                    # firma Ed25519
│  └─ README.md                       # como anadir modelos
│
├─ tests/                             # (Fases 2-6)
│  ├─ e2e/                            # Playwright + Electron
│  ├─ fixtures/                       # set de referencia foto/anime
│  ├─ golden/                         # hashes/SSIM esperados
│  └─ benchmarks/                     # harness comparativo vs Upscayl
│
├─ scripts/
│  ├─ build-main.mjs                  # esbuild: main + preload
│  ├─ dev.mjs                         # Next dev + espera + Electron
│  ├─ start.mjs                       # ejecuta el build ya hecho
│  ├─ clean.mjs
│  ├─ gen-openapi-client.mjs          # (Fase 4) OpenAPI -> TypeScript
│  ├─ fetch-models.mjs                # (Fase 4)
│  ├─ make-test-model.mjs             # (Fase 2) ONNX identidad x4 para CI
│  ├─ package.mjs                     # (Fase 5) electron-builder por plataforma
│  └─ benchmark.mjs                   # (Fase 5)
│
├─ docs/
│  ├─ 01-plan-de-proyecto.md
│  ├─ 02-arquitectura.md
│  ├─ 03-decisiones-adr.md
│  ├─ 04-modelos-y-pipelines.md
│  ├─ 05-guia-de-usuario.md           # (Fase 5)
│  ├─ 06-solucion-de-problemas.md     # (Fase 5)
│  └─ benchmarks/                     # (Fase 5)
│
└─ .github/workflows/                 # (Fase 5)
   ├─ ci.yml                          # lint + test + contract check
   ├─ build-sidecar.yml               # matriz de compilacion por plataforma
   └─ release.yml                     # firmado + notarizacion + publicacion
```

---

## 12. Logs y telemetría

### 12.1 Logs

- `tracing` + `tracing-subscriber` con salida a `logs/superupscaly-YYYY-MM-DD.log` (rotación diaria, retención 14 días) y a la consola en desarrollo.
- Formato: JSON estructurado en producción (`{"ts","level","target","msg","jobId","itemId","ep","tile"}`), legible en desarrollo.
- El renderer tiene un `LogViewer` que muestra los últimos 500 eventos y un botón "Exportar diagnóstico" que genera un ZIP con logs + `capabilities.json` + `settings.json` (sin rutas completas de usuario: se enmascaran a `<home>/…`).

### 12.2 Telemetría (opt-in)

- Pantalla de primer arranque con un interruptor **apagado por defecto**. Se puede cambiar en cualquier momento.
- Eventos: versión, plataforma, EP usado, modelo, resolución de entrada, tiempos, pico de VRAM, código de error, si hubo degradación.
- **Nunca** se envían: rutas de archivo, nombres de archivo, imágenes, texto EXIF, ni identificadores estables del equipo. El identificador de instalación es un UUID aleatorio rotado cada 90 días.
- Local-first: las métricas se agregan en `metrics.sqlite` y solo se envían si el usuario lo activa.
