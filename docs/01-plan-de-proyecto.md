# 01 · Plan de proyecto

> SuperUpscaly — Aplicación de escritorio de upscaling de imágenes.

---

## 1. Visión y propuesta de valor

Upscayl demostró que el upscaling local con IA es viable y popular, pero arrastra problemas estructurales conocidos: desbordamientos de VRAM con imágenes grandes, cuelgues o rutas vacías al arrastrar archivos (especialmente en Linux/Wayland), fallos silenciosos que producen imágenes negras, límites prácticos en el tamaño de lote y errores de Vulkan no gestionados.

**SuperUpscaly** parte de la misma promesa (simple, local, gratis) y la ejecuta con ingeniería de producto:

| Problema en Upscayl | Cómo lo resuelve SuperUpscaly |
|---|---|
| OOM de VRAM | Tiling adaptativo con presupuesto de VRAM medido + degradación progresiva + reintento |
| Imágenes negras / corruptas | Validación del buffer de salida antes de escribir; nunca se escribe un archivo inválido |
| Crash en drag & drop | `webUtils.getPathForFile()` (Electron ≥32), validación de rutas, fallback a selector de archivos |
| Fallos silenciosos | Taxonomía de errores con códigos, logs estructurados y mensajes accionables en la UI |
| Límite de lote | Cola persistente en SQLite, sin límite de tamaño, reanudable |
| Errores de Vulkan/EP sin manejar | Capa de abstracción de EP con detección, fallback en cascada y reporte explícito |
| Rendimiento | ONNX Runtime + TensorRT (objetivo: ≥2× throughput vs NCNN-Vulkan en NVIDIA) |

**Público:** usuarios avanzados y profesionales que escalan fotos y dibujos/anime y necesitan resultados de vanguardia sin depender de la nube.

**No-objetivos (explícitos):** edición de imagen no destructiva, corrección de color, gestión de biblioteca, plugins, versiones móviles o web, generación de imagen. Solo dos tipos de contenido: **Fotos** y **Dibujo/Anime**.

---

## 2. Alcance funcional

### 2.1 Dentro de alcance (v1.0)

1. UI de ventana única, tema morado, layout equivalente a Upscayl.
2. Drag & drop multiplataforma + selección de archivos/carpetas.
3. Dos modos excluyentes: **Fotos** y **Dibujo/Anime**.
4. Factores de escala: 2x, 4x, 8x.
5. Configuración avanzada colapsable: tile, dispositivo, cadena de modelos, restauración facial (solo Fotos).
6. Procesamiento por lotes con progreso individual y global, pausa/cancelación y reanudación.
7. Análisis pre-upscaling (rostros, ruido, resolución, tipo de contenido) con recomendación de modelo.
8. Gestor de modelos con manifiesto remoto simulado localmente, verificación por hash y caché.
9. Formatos de entrada JPG/PNG/WEBP/BMP/TIFF + ZIP/CBZ; salida PNG (defecto), JPG, WEBP.
10. Logs estructurados, exportables; telemetría opt-in.
11. Empaquetado Windows 10/11, macOS 12+, Linux AppImage/DEB.
12. i18n es/en (UI en español por defecto).

### 2.2 Fuera de alcance (v1.0)

- Procesamiento por GPU distribuido en red.
- Modelos de vídeo o secuencias.
- Edición interactiva del resultado (pinceles, máscaras manuales).
- Cuentas de usuario o sincronización en la nube.

---

## 3. Fases de entrega

Cada fase termina con un **criterio de salida verificable**. No se avanza sin validación explícita del usuario.

### Fase 0 — Plan y arquitectura ✅ *(este entregable)*

**Salida:** `README.md` + `docs/01..04`.
**Criterio de salida:** el usuario aprueba decisiones tecnológicas (Rust vs Python, protocolo, estrategia de escala) y el layout de UI propuesto.

---

### Fase 1 — Esqueleto del monorepo y UI estática ✅

**Entregado:**

- Monorepo **npm workspaces** con cuatro paquetes: `apps/desktop`, `apps/renderer`, `packages/shared`, `packages/ui` (ver ADR-016).
- `packages/shared`: tipos de dominio, contratos del puente (`SuApi`), catálogo de códigos de error, paleta, validadores de la frontera IPC y diccionario es/en tipado.
- `packages/ui`: primitivas (`Button`, `Card`, `Progress`, `SegmentedControl`, `Switch`, `Field`, `Tooltip`) con el tema morado como única fuente de color.
- `apps/desktop`: proceso principal completo — single-instance lock, gestión y persistencia del estado de ventana, **esquema `app://` con CSP** para servir el export estático, validación y canonicalización de rutas, expansión recursiva de carpetas, ajustes persistentes con escritura atómica, logs JSON con enmascarado del directorio personal, y ocho handlers IPC con comprobación de emisor.
- `apps/desktop/src/preload`: superficie mínima por `contextBridge`, incluido `webUtils.getPathForFile` para el drag & drop.
- `apps/renderer`: UI completa navegable — barra lateral con modo y escala, ajustes avanzados colapsables, zona de arrastrar y soltar, cola con progreso por imagen, selector de carpeta de salida, barra de acción con pausa/cancelación, diálogo de resumen, comparación antes y después con línea arrastrable (Fase 4) y sistema de avisos con código de error y acción sugerida.
- Drag & drop de extremo a extremo: `File` → ruta real → validación en el proceso principal → item de cola con tamaño en disco.
- Simulación del lote (`lib/mockRunner.ts`) que recorre **exactamente las mismas etapas** que recorrerá el sidecar, para validar estados, pausa, cancelación y resumen antes de que exista la inferencia.

**Desviaciones respecto al plan inicial** (documentadas en ADR-016 y ADR-017): npm workspaces en lugar de pnpm; sin `turbo` (con dos tareas de build no aporta caché útil); sin `zod` ni `i18next`; el proceso principal y el preload se empaquetan con esbuild.

**Criterio de salida:** ✅ verificado — `npm run typecheck` y `npm run build` pasan; la app arranca y acepta arrastrar archivos con rutas reales.

---

### Fase 2 — Sidecar de inferencia (MVP headless) ✅ *(pendiente de verificar en Linux)*

El workspace Cargo tiene **once crates**. `su-core` y `su-tiling` se compilaron y pasaron sus tests en el entorno de desarrollo; el resto está escrito y pendiente de compilar en Linux.

| Crate | Estado | Tests |
|---|---|---|
| `su-core` | ✅ compilado y verificado | **31** |
| `su-tiling` | ✅ compilado y verificado | **50** |
| `su-telemetry` | escrito | 9 |
| `su-hardware` | escrito | 20 |
| `su-imageio` | escrito | 25 |
| `su-models` | escrito | 20 |
| `su-analyze` | escrito | 12 |
| `su-inference` | escrito | 32 |
| `su-jobs` | escrito | 32 |
| `su-server` | escrito | 8 |
| `su-cli` | escrito | 7 |
| **Total** | | **246** |

- **`su-core`**: tipos de dominio con invariantes en el sistema de tipos (un `scale` de 3 es imposible de construir), taxonomía de errores alineada con `packages/shared/src/error-codes.ts`, y el **motor de pipelines** con su AST restringido: condiciones `and`/`or`/`not`/comparación, compiladas y validadas en tiempo de carga. Una variable mal escrita en un `pipelines.user.json` es un error explícito, no un `false` silencioso.
- **`su-tiling`**: planificación de tiles, ventana coseno con demostración de que los pesos suman exactamente 1 en los solapes, compositor con acumulación ponderada, presupuesto de VRAM por execution provider y escalera de degradación.
- **`su-imageio`**: decodificación con orientación EXIF aplicada antes de inferir, canal alfa tratado aparte —y escalado por el mismo modelo que el color, ADR-029—, reescalado en `f32` con kernel elegible, **escritura atómica** y **validación de salida**.
- **`su-models`**: manifiesto validado, verificación SHA-256 por bloques y distinción explícita entre `Installed`, `HashMismatch`, `Unverified` y `Missing`.
- **`su-analyze`**: estimadores de ruido (laplaciano) y artefactos de compresión (rejilla 8×8), ambos puros y verificables con imágenes sintéticas. La detección de rostros y el clasificador de contenido llegan en la Fase 4.
- **`su-inference`**: abstracción `Backend`, runner que une pipelines + tiling + degradación, y un `MockBackend` que permite verificar toda la cadena **sin ONNX Runtime, sin GPU y sin modelos**. ORT queda detrás de la feature `onnx`.
- **`su-jobs`**: cola de trabajos, hilo propio por trabajo, pausa y cancelación cooperativas en puntos seguros, y flujo de eventos con coalescencia de progreso.
- **`su-server`**: API HTTP + WebSocket, token comparado en tiempo constante, rechazo de `Host` no local y *portfile* atómico.
- **`su-cli`**: `upscale`, `capabilities`, `models`, `pipelines` y `serve`. El comando `upscale` crea un trabajo real y se suscribe a sus eventos, así que ejercita el mismo camino que la interfaz.

**Verificación definitiva** (en Linux, donde el toolchain funciona de forma nativa):

```bash
cd services/inference && cargo test
cargo run -p su-cli -- capabilities
cargo run -p su-cli -- upscale foto.png --output ./salida --scale 4
```

**Criterio de salida:**
- ⏳ `su-cli` escala una imagen 512×512 ×4 (funciona con el backend de referencia; la ruta con modelo real ya compila y está conectada, pero no se ha ejecutado nunca con un modelo: falta el binario de ONNX Runtime)
- ✅ `GET /v1/capabilities` reporta EPs, GPU, VRAM y modelos disponibles
- ✅ Tests unitarios de `su-tiling` en verde (50 tests)

---

### Fase 3 — Tiling adaptativo, VRAM y pipelines 🔄 en curso

**Entregado:**

- **Calibración de VRAM y tiempo por `(modelo, EP, dispositivo, tile)`** (`su-tiling::calibration`). Los valores del manifiesto son estimaciones escritas a mano; tras la primera ejecución de un modelo en un equipo, la elección de tile deja de ser una conjetura. Tres decisiones que importan:
  - **Se suaviza, no se reemplaza.** Una media exponencial con peso decreciente hace que el valor converja en lugar de oscilar con cada medición.
  - **Se rechazan los valores absurdos.** Una muestra que se aleja más de 4× del valor actual se descarta: es más probable que sea otra aplicación usando la GPU que un cambio real de consumo. Aceptarla envenenaría la calibración de ese modelo para siempre.
  - **Sin datos no se inventa nada.** Si no hay medición, se usa la estimación del manifiesto.
- **Persistencia de trabajos en SQLite** (`su-jobs::store`), con reanudación.
  - **Una sola tabla con el trabajo en JSON**, no columnas más payload. Con dos fuentes de verdad, un `UPDATE` de estado que no toque el payload deja la base de datos mintiendo, y ese fallo aparece justo al reanudar tras un cierre inesperado.
  - **Se guarda en cuatro momentos**, no en cada evento de progreso: al crear, al terminar cada imagen y al cerrar el lote. Un lote con cientos de tiles generaría miles de escrituras por imagen si se guardara tile a tile.
  - **Un fallo al persistir no detiene el lote.** Se registra y el trabajo continúa: perder el registro es malo, matar un trabajo que el usuario espera porque el disco se llenó es peor.
  - **Reanudar es una sola ruta**, no dos. El bucle de ejecución se salta los ítems ya resueltos y siembra los contadores con lo que había, así que el arranque y la reanudación comparten código en lugar de mantener dos versiones parecidas que se desincronizan.
  - **Identificadores con marca de tiempo**, únicos entre reinicios sin restaurar un contador desde el disco.

- **Backend de ONNX Runtime** (`su-inference::ort_backend`), detrás de la feature `onnx`.
  - **`load-dynamic`**: no se enlaza ORT al compilar; el binario se carga en tiempo de ejecución. Sin eso no se pueden ofrecer los aceleradores de NVIDIA como descarga aparte (ADR-012).
  - **Sin `download-binaries`**: ONNX Runtime lo aporta la aplicación, no el crate. Activarlo haría que `cargo build --features onnx` intentara descargar cientos de megabytes.
  - **Sin `ndarray`**: la entrada se construye con `(shape, Vec<f32>)` y la salida se lee con `try_extract_tensor`, que devuelve `&[f32]`. Para pasar tiles de imagen no hace falta nada más.
  - **La conversión `HWC` ↔ `NCHW` está aislada en dos funciones puras con tests propios.** Un error ahí produce una imagen con los canales intercambiados: un fallo que se ve pero se diagnostica mal.
  - **Solo los errores de memoria se marcan como recuperables.** Un operador no soportado no se arregla reduciendo el tile, así que no dispara la escalera de degradación: falla con un mensaje claro.
  - **Caché de sesiones por modelo.** Cargar 67 MB en cada imagen de un lote de cien convertiría un trabajo de minutos en uno de horas.

**Pendiente de esta fase:**
- Reanudación a **nivel de tile** para imágenes grandes (hoy la reanudación es a nivel de imagen).
- Sustituir `imageops` por `fast_image_resize` en el reescalado (SIMD).

**Criterio de salida:**
- Procesar una imagen de 8192×8192 ×4 en una GPU con 4 GB de VRAM sin OOM y sin imagen negra.
- Matar el proceso sidecar a mitad de un lote → al relanzar, el lote continúa desde donde quedó.
- `cargo test` completo en verde, incluidos los tests de OOM simulado (tile forzado demasiado grande).

---

### Fase 4 — Integración UI ↔ backend y modos 🔄 en curso

**Entregado (fontanería Electron ↔ sidecar):**

- **`sidecar/locator.ts`**: búsqueda del ejecutable por plataforma y arquitectura, con `candidatePaths` como función pura para poder testearla sin tener el binario construido. Se puede forzar una ruta con `SU_SIDECAR_BIN`.
- **`sidecar/supervisor.ts`**: ciclo de vida completo.
  - **El token va por entorno, nunca por argumentos**: los argumentos de un proceso son visibles para cualquier otro proceso del equipo.
  - **El portfile se valida contra el PID del hijo.** Un `runtime.json` de una sesión anterior se leería al instante y apuntaría a un puerto muerto; comparar el PID es más fiable que fiarse de la marca de tiempo del archivo.
  - **Reinicio con backoff exponencial** (1 s → 30 s) y rendición tras 5 intentos, con aviso. Si el sidecar muere porque falta una biblioteca, reintentar en bucle solo llena el log.
  - **Cierre ordenado**: `POST /v1/shutdown`, margen, y solo entonces la señal. Matarlo directamente dejaría el trabajo en curso sin marcar como interrumpido y la reanudación no lo ofrecería.
  - **Comprobación de versión de protocolo** al arrancar: un sidecar viejo se detecta ahí, no a mitad de un lote.
- **`sidecar/client.ts`**: cliente HTTP tipado y flujo de eventos por WebSocket con reconexión automática. Se usa `ws` en lugar del `WebSocket` global de Node porque la API del estándar no permite enviar cabeceras, y el token viaja en `Authorization` — pasarlo por la URL lo dejaría en los logs.
- **`ipc/sidecar.ts`**: puente con validación **que lanza error** en lugar de descartar campos en silencio. Un ajuste descartado no cambia nada visible; un campo de un trabajo descartado hace que el trabajo haga algo distinto de lo que el usuario pidió.
- **`ipc/handle.ts`**: el registro de manejadores con comprobación de emisor y registro de errores se extrae para que sea inevitable, no opcional por manejador.

**Entregado (la interfaz ya usa el motor real):**

- **`state/useRun.ts`** envía un trabajo de verdad y lo controla (pausar, reanudar, cancelar). **Se eliminó el simulador de progreso.** Su problema no era ser falso, era lo que hacía al faltar el motor: mostraba una barra avanzando y un resumen de "N completadas" sin haber procesado nada, con la carpeta de salida vacía y sin explicación. Si el motor no está, ahora se dice con el motivo concreto, incluido el comando para compilarlo.
- **`state/useSidecarEvents.ts`** traduce los eventos en estado de interfaz. Los ítems se marcan como terminados **porque el motor lo dice**, no porque la interfaz lo suponga. Los ítems se emparejan por ruta de origen, no por índice: emparejar por índice funcionaría hasta que una reanudación se saltara una imagen ya procesada, y entonces todo se desplazaría.
- **`components/EngineStatus.tsx`**: el estado del motor está visible **siempre**, no solo cuando falla. Un usuario que no entiende por qué el botón no hace nada necesita verlo antes de pulsarlo.
- La suscripción a los eventos vive en la página, no en un componente que se monta y desmonta: cada ciclo dejaría un hueco en el que se perderían eventos.

**Pendiente de esta fase:**
- Selección automática de la cadena de modelos por modo y análisis, con selector Automático/Manual.
- ~~Restauración facial opcional (solo Fotos) con control de intensidad.~~ **Hecho** (ADR-032): la etapa recorta cada cara, la restaura y la pega con máscara radial, con la intensidad de la preferencia. **Falta el detector**: el análisis devuelve `faces` vacía, así que la etapa se omite con su motivo. El motor ya consume las cajas (`FaceBox`), de modo que añadir el detector no requiere tocarlo.
- Análisis completo: **detección de rostros** (lo que bloquea la línea anterior) y clasificador de contenido.
- Gestor de modelos con descarga y firma.
- Logs exportables y telemetría opt-in.

**Criterio de salida:**
- Un lote de 100 imágenes mixtas (fotos + anime, varias resoluciones) se completa sin intervención, con salida correcta para todas.
- Pulsar "Cancelar" detiene el trabajo en < 2 s y deja el estado consistente.
- Desconectar/reconectar la GPU a mitad de trabajo produce un error claro, no un cuelgue.

---

### Fase 5 — Calidad, rendimiento y empaquetado 🔄 en curso

**Entregado:**

- **Documentación completa**: [guía de usuario](05-guia-de-usuario.md), [solución de problemas](06-solucion-de-problemas.md), [guía de contribución](../CONTRIBUTING.md) y [LICENSE](../LICENSE).
  - La guía de usuario explica **cuándo conviene cambiar cada ajuste**, no solo qué hace. Un desplegable sin criterio es peor que no tenerlo.
  - La guía de problemas está ordenada **por síntoma**, no por causa, porque es así como llega el usuario. Incluye los errores de compilación conocidos (falta de `binutils` en el target GNU, compilador de C para SQLite) y los específicos de cada plataforma.
  - La licencia separa explícitamente **el código (MIT) de los modelos** (licencia de cada autor), con la tabla de restricciones de uso comercial. `CodeFormer` y `2x-AnimeSharpV3` no permiten uso comercial y así se avisa.
- **Empaquetado** (`apps/desktop/electron-builder.yml` + `scripts/package.mjs`).
  - El renderer va en `extraResources`, no dentro del asar: se sirve desde el esquema `app://` y no puede vivir en un archivo comprimido.
  - El binario del sidecar se separa por plataforma y arquitectura, y va en `asarUnpack` porque **un ejecutable no puede lanzarse desde dentro del asar**.
  - macOS con *hardened runtime* y entitlements compartidos entre la app, el sidecar y las bibliotecas de ORT. Sin eso, macOS mata el proceso hijo al arrancar.
  - El DEB aplica SUID a `chrome-sandbox` desde el `postinst`. En AppImage **no** se activa `--no-sandbox` por defecto: desactivar el sandbox de Chromium para todo el mundo porque una minoría tenga el SUID mal puesto es un mal intercambio.
- **ADR-014 completado**: el EP de CPU usa `with_intra_threads = núcleos físicos` e `with_inter_threads = 1`. Firmas verificadas leyendo el código fuente de `ort`.
- **Harness de benchmark** (`scripts/benchmark.mjs`, `npm run benchmark`), con set de referencia en `tests/fixtures`.
  - **Mide tiempo, no calidad**, y lo declara en su cabecera. Mezclar las dos cosas en un solo número haría que una regresión de calidad se leyera como una mejora de rendimiento.
  - **Sin `--upscayl-bin` no inventa comparación**: publica los tiempos propios y calla sobre la aceleración, en lugar de medirse contra un objetivo fijado en otra máquina.
  - Compara medianas y p95 y emite el informe en Markdown, con la aceleración contrastada contra el objetivo de AC-10.

**Pendiente de esta fase:**
- Golden tests de calidad (SSIM/LPIPS) con umbrales de no-regresión en CI. El harness de benchmark **no** los cubre: mide tiempo, no calidad.
- "Acceleration Packs" descargables.
- Firma de código y notarización.

**Criterio de salida:**
- Instaladores funcionando en las tres plataformas, con verificación en máquina limpia.
- Informe de benchmark publicado en `docs/benchmarks/` con la comparativa vs Upscayl.

---

### Fase 6 — QA y estabilidad

**Trabajo:**
- E2E con Playwright + Electron.
- Pruebas de estrés: 500 ciclos de drag & drop, lote de 1000 imágenes, 20 imágenes de 100 MP.
- Matriz manual de dispositivos documentada (ver §6).
- Corrección de los defectos encontrados y cierre de la v1.0.

**Criterio de salida:** todos los criterios de aceptación de §5 verificados y documentados.

---

## 4. Estructura de equipo (roles que asume el agente)

| Rol | Responsabilidad en el proyecto |
|---|---|
| Arquitecto de software | Contratos entre capas, estructura del monorepo, evolución de la API |
| Ingeniero ML/inferencia | Selección de modelos, EPs, tiling, VRAM, pipelines, benchmarks de calidad |
| Desarrollador full-stack | Electron main/preload, Next.js, Jotai, Tailwind, cliente tipado |
| Especialista UX/UI | Paridad de layout con Upscayl, paleta morada, accesibilidad, estados vacíos/error |
| QA engineer | Plan de pruebas, matriz de dispositivos, harness de estrés, criterios de aceptación |

---

## 5. Criterios de aceptación (medibles)

### 5.1 Funcionales

| ID | Criterio | Verificación |
|---|---|---|
| AC-01 | La app se llama **SuperUpscaly** en título, instalador, `package.json` y binarios | Inspección |
| AC-02 | La UI replica el layout de Upscayl con paleta morada | Diff visual lado a lado |
| AC-03 | Solo existen dos modos: Fotos y Dibujo/Anime | Test E2E |
| AC-04 | Escalas 2x, 4x, 8x producen dimensiones exactas `round(w·s)` × `round(h·s)` | Test unitario + E2E |
| AC-05 | Drag & drop funciona en Windows, macOS y Linux (X11 y Wayland) | Matriz manual |
| AC-06 | Un lote de 1000 imágenes se completa sin límite artificial | Test de estrés |
| AC-07 | Reanudación: matar el sidecar a mitad de lote y relanzar continúa el trabajo | Test de integración |
| AC-08 | Al finalizar un lote se muestra resumen con éxitos, fallos y tiempos | Test E2E |

### 5.2 Rendimiento

Metodología: 20 imágenes de referencia (10 foto, 10 ilustración), 1024×1024 y 2048×2048, ×4, 3 repeticiones, misma máquina.

| ID | Criterio |
|---|---|
| AC-10 | `throughput(SuperUpscaly, TensorRT) / throughput(Upscayl, NCNN-Vulkan) ≥ 2.0` en p50 y `≥ 1.6` en p95 |
| AC-11 | Con CUDA EP (sin TRT): ratio `≥ 1.2` en p50 |
| AC-12 | En CPU se usan todos los núcleos físicos (`intra_op_num_threads = núcleos físicos`) |
| AC-13 | Pico de VRAM en 2048×2048 ×4 ≤ 80 % de la VRAM libre inicial |

> Los valores absolutos de tiempo no se fijan a priori: la referencia es siempre **medida**, no asumida, para que el criterio siga siendo válido en hardware futuro.

### 5.3 Calidad

| ID | Criterio |
|---|---|
| AC-20 | `SSIM(SuperUpscaly) ≥ SSIM(Upscayl) − 0.005` en 4x sobre el set de referencia |
| AC-21 | `LPIPS(SuperUpscaly) ≤ LPIPS(Upscayl)` en 4x |
| AC-22 | En 8x, SSIM y LPIPS **estrictamente mejores** que Upscayl (doble pasada vs pasada única) |
| AC-23 | 0 imágenes negras o corruptas en un lote de 1000 |

### 5.4 Estabilidad

| ID | Criterio |
|---|---|
| AC-30 | 500 ciclos de drag & drop → 0 crashes, 0 rutas vacías sin mensaje |
| AC-31 | 20 imágenes de ≥100 MP procesadas sin OOM y sin fallo |
| AC-32 | Falta de VRAM, archivo corrupto, formato no soportado y pérdida de dispositivo producen mensaje claro + log, nunca cierre silencioso |
| AC-33 | Matar el proceso sidecar con SIGKILL → la app se recupera y lo relanza automáticamente |

### 5.5 Documentación

| ID | Criterio |
|---|---|
| AC-40 | Un desarrollador ajeno puede compilar y ejecutar el proyecto siguiendo el README |
| AC-41 | Existe guía para añadir modelos sin tocar código |
| AC-42 | Existe guía de solución de problemas con los 10 errores más comunes |

---

## 6. Matriz de verificación de dispositivos

| Plataforma | CPU | NVIDIA | AMD | Intel | Apple |
|---|---|---|---|---|---|
| Windows 10/11 | ✅ requerido | TRT + CUDA | DirectML | DirectML | — |
| macOS 12+ | ✅ requerido | — | — | — | CoreML (M1–M4) |
| Linux x64 | ✅ requerido | TRT + CUDA | Vulkan→CPU fallback | CPU | — |

**Matriz mínima de pruebas manuales (Fase 6):** 1 equipo Windows+NVIDIA, 1 Windows+AMD, 1 macOS Apple Silicon, 1 Linux+NVIDIA, 1 Linux solo-CPU (VM).

---

## 7. Riesgos y mitigaciones

| # | Riesgo | Impacto | Prob. | Mitigación |
|---|---|---|---|---|
| R-01 | Los motores TensorRT se invalidan al cambiar driver/versión de ORT | Alto | Media | Caché de motores versionada por `(driver, ort, modelo, shape)`; reconstrucción automática con aviso y timing visible |
| R-02 | DirectML limita formas dinámicas → necesidad de recompilar por tamaño de tile | Medio | Alta | Precompilar un conjunto discreto de tiles (256/384/512/768/1024) en el primer uso y cachear |
| R-03 | Modelos >2 GB no cargan en GPU con poca VRAM | Medio | Media | `unloadBetweenImages`, tiles pequeños, fallback a CPU, aviso en UI antes de empezar |
| R-04 | CoreML no soporta todos los operadores de los modelos x4 | Medio | Media | Grafo de fallback de operadores a CPU dentro de la misma sesión; validación en CI macOS |
| R-05 | AppImage: `chrome-sandbox` requiere SUID | Medio | Alta | En DEB: `postinst` con `chown root:root` + `chmod 4755`; en AppImage: documentar y usar `--no-sandbox` solo si el usuario lo pide |
| R-06 | Firma/notarización macOS con binario sidecar y ORT | Alto | Media | Firmar cada binario y `.dylib` de ORT con hardened runtime + entitlements; verificar con `codesign --verify --deep --strict` |
| R-07 | Licencias de modelos (Real-ESRGAN BSD-3, GFPGAN Apache-2.0, HAT Apache-2.0, CodeFormer S-Lab) | Medio | Baja | Descarga en el primer uso, nunca bundleados; pantalla de licencias en el gestor de modelos |
| R-08 | Antivirus bloquea binario Rust recién descargado | Bajo | Media | Firma de código; publicación con reputación acumulada; sin auto-extracción en temp |
| R-09 | 8x con doble pasada genera artefactos o tiempos inaceptables | Medio | Media | Interpolación intermedia controlada, aviso de tiempo estimado, opción de 8x en dos pasos manuales |
| R-10 | Discrepancia de color por ICC/EXIF ignorado | Medio | Media | Aplicar orientación EXIF antes de inferir; preservar perfil ICC en PNG/WEBP |
| R-11 | Rutas largas en Windows (>260) | Bajo | Media | Prefijo `\\?\` al pasar rutas al sidecar |
| R-12 | Rutas inaccesibles en Wayland (portal XDG) | Medio | Alta | `getPathForFile`, detección de path vacío → fallback a selector nativo + mensaje claro |

---

## 8. Matriz de trazabilidad requisito → fase

| Requisito del brief | Fase | Documento |
|---|---|---|
| Plan y arquitectura | 0 | 01, 02, 03 |
| UI morada idéntica a Upscayl | 1 | 02 §6 |
| Drag & drop multiplataforma | 1 | 02 §9 |
| Dos modos de upscaling | 1 + 4 | 02 §6, 04 §3 |
| Backend ONNX Runtime + EPs | 2 + 3 | 02 §3, 03 |
| Tiling adaptativo y VRAM | 3 | 02 §4 |
| Gestión de VRAM y paralelismo | 3 | 02 §4.5 |
| Formatos + ZIP/CBZ | 2 + 3 | 02 §5 |
| Gestor de modelos | 2 + 4 | 04 |
| Análisis pre-upscaling | 4 | 02 §7 |
| Lotes, cola, reanudación | 3 + 4 | 02 §8 |
| Rendimiento ≥2× vs Upscayl | 5 | 01 §5.2 |
| Empaquetado multiplataforma | 5 | 01 §3, 03 ADR-012 |
| Tests unitarios/integración/manuales | 2–6 | 01 §6, 02 §11 |
| Documentación y licencia | 5 | README, docs |

---

## 9. Próximos pasos inmediatos

1. ~~**Verificar el sidecar en Linux**~~ **Hecho.** `cd services/inference && cargo test` pasa: 344 pruebas sin la feature `onnx` y **363 con `--all-features`**, sin un solo aviso de compilación. En TypeScript, `npm run typecheck` pasa en los cuatro paquetes y `npm test` (88 pruebas) también. En el entorno donde se escribió el sidecar solo se había llegado a `cargo check --workspace --all-targets` **y a `cargo check -p su-cli --features onnx --all-targets`**, porque enlazar exigía `dlltool` y un compilador de C que ese entorno no tenía. La primera ejecución real de las pruebas encontró **siete defectos de producción**: están corregidos y documentados en ADR-022.
2. **Fase 3**: reanudación a nivel de tile y el reescalado con SIMD (`fast_image_resize`). La persistencia en SQLite, la calibración de VRAM y el backend de ONNX Runtime ya están entregados.
3. **Fase 4**: firma del manifiesto (la **descarga** ya está entregada — el sidecar la describe y la aplicación la ejecuta, ver ADR-019; lo que falta es la firma Ed25519 y su verificación), **detección de rostros** (es lo único que separa a la restauración facial de funcionar: `su-analyze` solo estima ruido y artefactos de compresión, y el detector `yunet-2023` no está en el catálogo embebido) y clasificador de contenido, y logs exportables. La **entrada** en ZIP/CBZ ya funciona (ADR-019); la **salida** en ZIP no: `zip_output` está declarado en `su-core` y no se lee en ninguna parte, y ningún control de la interfaz lo ofrece.
4. ~~**Primera ejecución con un modelo real**~~ **Hecho** (ADR-025). El backend de ONNX Runtime se ha ejecutado con pesos reales (`realesrgan-x4plus-anime-6b`, hash verificado contra el manifiesto): 128 px → 512 px en **2,9 s en CPU**, con una transición de borde de 2,2 px frente a los 2,6 px de la referencia que trajo el usuario, y sin errores en la interfaz. Hizo falta la biblioteca de ONNX Runtime **1.28** (la 1.22.2 que traía el sistema no vale para `ort` 2.0-rc.13) y comprobar que sin ella el motor degrada en lugar de morir.
5. **Descargar el runtime de ONNX Runtime desde la aplicación**, como ya se hace con los modelos. Hoy hay que copiarlo a mano (`docs/06`). Y, cuando haya biblioteca con CUDA y cuDNN disponibles, usar la GPU: en este equipo hay una RTX 4060 parada y la inferencia va en CPU.

> **Cerrado en la revisión del pliego.** Los cuatro ajustes de la interfaz que el motor ignoraba (`device`, `concurrency`, `unloadBetweenImages` y `priority`) ya hacen lo que dicen (ADR-021), la telemetría del sidecar está conectada, y hay 11 pruebas de integración que levantan el servidor real por socket.

> Cada fase se entrega y se valida antes de empezar la siguiente.
