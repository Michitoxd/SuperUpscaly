# 03 · Decisiones técnicas (ADR)

Formato: **Contexto → Opciones → Decisión → Consecuencias**. Cada decisión es revisable: si en el futuro una restricción cambia, se añade un ADR de supersesión en lugar de reescribir el original.

---

## ADR-001 — Lenguaje del backend de inferencia: **Rust**

**Contexto.** El brief permite Python (FastAPI + ONNX Runtime) o C++/Rust con ONNX Runtime, y pide elegir la opción con mejor rendimiento y facilidad de integración, documentando la decisión.

**Opciones evaluadas**

| Criterio | Rust + `ort` | Python + FastAPI | C++ + ORT C API |
|---|---|---|---|
| Latencia por petición | ~0.2 ms | ~3–8 ms | ~0.2 ms |
| Distribución | Binario único estático (~15–40 MB) | PyInstaller ~150–250 MB, arranque 1.5–4 s | Binario + DLLs, ~30 MB |
| Falsos positivos de antivirus | Raros | **Frecuentes** con PyInstaller (problema real en herramientas similares) | Raros |
| Control de memoria/VRAM | Determinista, sin GC | GC + allocator no deterministas | Determinista |
| Concurrencia | `tokio` async real, sin GIL | GIL, requiere `run_in_executor` | Manual |
| Cobertura de EPs de ORT | TRT, CUDA, DirectML, CoreML, CPU, (Vulkan vía OpenVINO) | TRT, CUDA, DirectML, CoreML, CPU, OpenVINO | Todos |
| Facilidad de escritura | Media | Alta | Baja |
| Facilidad de empaquetado multiplataforma | Alta (cross-compile, matriz CI) | Media (muchos binarios nativos) | Baja |
| Riesgo de bugs de memoria | Nulo (safe Rust) | Nulo | Alto |

**Decisión.** **Rust** con el crate `ort` (bindings oficiales de ONNX Runtime) en modo **carga dinámica** (`load-dynamic`).

**Justificación.** El rendimiento de inferencia lo determina el EP de ONNX Runtime, no el lenguaje anfitrión; la diferencia real está en el *overhead por imagen*, la *distribución* y la *fiabilidad*. Rust gana en los tres: sin intérprete embebido, arranque instantáneo, control explícito de VRAM (crítico para el tiling adaptativo), concurrencia sin GIL, y un único binario por plataforma que se firma y notariza sin fricción. La carga dinámica permite además enviar varias compilaciones de ORT (una por familia de EP) y elegir en tiempo de ejecución, en lugar de imponer la peor combinación a todos los usuarios.

**Consecuencias**

- (+) Instalador base pequeño; "Acceleration Packs" descargables por separado (ver ADR-012).
- (+) Arranque del sidecar < 100 ms.
- (+) El `SessionPool`, el presupuesto de VRAM y el pipeline asíncrono se implementan sin adaptadores.
- (−) Más código para lo mismo (compresión de imagen, ZIP, SQLite ya tienen crates maduras, así que el coste es acotado).
- (−) Requiere toolchain de Rust en el CI y en las máquinas de desarrollo.
- (−) El crate `ort` exige fijar la versión de ONNX Runtime; se documenta y se verifica en CI con un test de humo por EP.

---

## ADR-002 — Runtime de inferencia: **solo ONNX Runtime**; NCNN únicamente como último recurso

**Contexto.** Upscayl usa binarios NCNN-Vulkan. El brief pide ONNX Runtime con TensorRT → CUDA → DirectML → CoreML → CPU, y NCNN-Vulkan solo como fallback opcional.

**Decisión.** ONNX Runtime como único runtime. NCNN-Vulkan **no se empaqueta**: se documenta como vía de último recurso (compilación opcional de `su-server` con la feature `ncnn-fallback`) y no forma parte de los instaladores por defecto.

**Justificación.** Mantener un segundo runtime duplica el catálogo de modelos, la lógica de tiling y la matriz de pruebas. NCNN aporta valor solo donde Vulkan funciona mejor que CPU — y ese hueco lo cubre DirectML en Windows y CPU en Linux. Los errores de Vulkan sin manejar son, además, uno de los defectos conocidos que este proyecto quiere eliminar: incluirlo reintroduciría esa superficie de fallo.

**Consecuencias**

- (+) Una sola ruta de código, una sola matriz de pruebas, un solo formato de modelo.
- (+) Se eliminan por diseño los crashes por driver Vulkan.
- (−) En Linux con GPU AMD/Intel sin ROCm/oneAPI, el rendimiento cae a CPU. Se mitiga con documentación clara y aviso en `capabilities`.
- (−) Los modelos NCNN populares deben convertirse a ONNX (proceso documentado en `04-modelos-y-pipelines.md`).

---

## ADR-003 — Comunicación UI ↔ backend: **HTTP/1.1 loopback + WebSocket**

**Contexto.** El brief menciona gRPC o REST. Las opciones reales son: IPC de Electron con `child_process` + stdio, HTTP local, gRPC, o named pipes.

**Decisión.** **HTTP/1.1 sobre 127.0.0.1 en puerto efímero** con token Bearer, más **WebSocket** para el stream de eventos. Sin gRPC.

**Justificación**

- Los píxeles **no se transfieren** (§1 de `02-arquitectura.md`): los mensajes son rutas y metadatos, de unos pocos KB. La eficiencia de gRPC/HTTP2 es irrelevante en este régimen.
- gRPC exige `protoc`, codegen en dos lenguajes y gestión de HTTP/2 sobre loopback; a cambio aporta streaming bidireccional que aquí se resuelve con un WebSocket de 20 líneas.
- HTTP hace el sidecar **testeable con `curl`**, depurable con herramientas estándar y usable por `su-cli` y por scripts de CI sin acoplarse a Electron. Esto es lo que hace posible el harness de benchmark.
- El esquema OpenAPI generado desde Rust da tipos TypeScript verificados en CI, lo que sustituye la ventaja principal de gRPC (contrato tipado).

**Consecuencias**

- (+) Sidecar independiente y probable por separado; se puede ejecutar sin la app.
- (+) Depuración trivial; logs de peticiones legibles.
- (−) Hay que implementar autenticación propia (token + validación de `Host`/`Origin`), porque un puerto local es alcanzable por otros procesos del equipo. Se mitiga en §2.2 del documento de arquitectura.
- (−) El puerto es efímero, así que hace falta el mecanismo de *portfile*.

---

## ADR-004 — Renderer: **Next.js con `output: 'export'`**

**Contexto.** El stack obligatorio incluye Next.js, pero Electron carga la UI desde `file://` o desde un servidor embebido.

**Decisión.** **Export estático** (`output: 'export'`, `images.unoptimized: true`) cargado desde `file://` en producción; **dev server de Next** en desarrollo (`ELECTRON_RENDERER_URL`).

**Justificación.** Un export estático elimina la necesidad de arrancar un servidor Node para la UI, reduce el consumo de memoria y simplifica el empaquetado. No se pierde nada: no hay SSR ni rutas dinámicas de servidor en una app de escritorio. En desarrollo se usa el dev server para conservar HMR.

**Consecuencias**

- (+) Un solo proceso menos en producción; arranque más rápido.
- (+) CSP más estricta (`script-src 'self'`).
- (−) Hay que evitar API de Next que requieran servidor (route handlers, `next/image` con optimización, ISR). Se documenta como regla de lint.
- (−) Los assets se referencian con rutas relativas; requiere `assetPrefix` correcto y verificación en CI.

---

## ADR-005 — Estado del renderer: **Jotai**

**Contexto.** El brief exige Jotai.

**Decisión.** Jotai, con átomos por dominio y sin store global mutable.

**Justificación y organización.** Los átomos se agrupan en cinco dominios: `settings` (persistido), `queue` (imágenes seleccionadas), `jobs` (trabajos en curso, alimentados por WS), `capabilities` (hardware, de solo lectura tras el arranque) y `ui` (modales, toasts, panel avanzado). El estado del sidecar **nunca se duplica**: es la fuente de verdad y los átomos son una proyección del último evento recibido. Esto evita la clase de bug más común en este tipo de apps: UI y backend mostrando progresos distintos.

**Consecuencias**

- (+) Re-render granular: la barra de progreso de un item no re-renderiza la lista completa.
- (−) Los átomos derivados de colecciones requieren `atomFamily` y memoización cuidadosa; se documenta el patrón en `packages/ui`.

---

## ADR-006 — Model chaining declarativo en JSON, no código

**Contexto.** El brief pide cadenas como `denoise → upscale → face restore → sharpen`, con selección automática o manual.

**Decisión.** Los pipelines se declaran en `pipelines.json` (empaquetado + sobrescribible por el usuario en `<appData>/pipelines.user.json`), con etapas tipadas y condiciones sobre `analysis.*`, `prefs.*` y `hardware.*`.

**Justificación.** Permite ajustar la calidad sin recompilar ni publicar una versión nueva; hace los pipelines testeables de forma aislada; y permite que la UI los muestre y edite (modo Manual) sin conocer detalles de implementación.

**Seguridad.** Las condiciones **no** se evalúan con un evaluador de expresiones genérico (nada de `eval`, `Function`, ni un motor de scripting embebido): se compilan a un AST propio con un conjunto cerrado de operadores (`&&`, `||`, `!`, comparadores, literales y accesos a variables). Un archivo de usuario con sintaxis inválida se rechaza con un error claro y se cae al pipeline por defecto.

**Consecuencias**

- (+) Extensible por el usuario sin riesgo de ejecución arbitraria.
- (−) Las condiciones complejas (bucles, lógica estadística) no son expresables; si se necesitan, se añade una etapa en Rust y se expone como variable.

---

## ADR-007 — Tiling adaptativo con presupuesto de VRAM y degradación progresiva

**Contexto.** El desbordamiento de VRAM es el defecto más citado de Upscayl. El tiling fijo con un desplegable (como en Upscayl) obliga al usuario a adivinar.

**Decisión.** Tile **automático por defecto**, calculado a partir de VRAM libre medida, un factor de seguridad, los pesos del modelo y una calibración empírica por `(modelo, EP, dispositivo, tile)`. El desplegable manual sigue existiendo en Configuración avanzada para usuarios expertos.

**Justificación.** El usuario no puede conocer la VRAM libre real ni el consumo por megapíxel de cada modelo. La calibración se aprende en la primera ejecución y a partir de ahí la elección es predictiva en lugar de heurística. La degradación progresiva garantiza que un error de estimación se traduzca en "más lento", nunca en "fallo" o "imagen negra".

**Consecuencias**

- (+) Cero OOM por diseño; el peor caso es un fallback a CPU con aviso explícito.
- (+) La primera ejecución de un modelo en un equipo es ligeramente conservadora (subestima el tile) hasta calibrar.
- (−) Requiere acceso a NVML/DXGI/Metal; en CPU o si falla la consulta, se usan valores conservadores fijos.

---

## ADR-008 — Escalas 2x y 8x derivadas de modelos 4x

**Contexto.** Casi todos los modelos de calidad disponibles son x4. Existen pocos x2 y los x8 no son prácticos.

**Decisión.**

- **2x** = modelo x4 + reescalado Lanczos3 a 0.5 con precisión de 16 bits por canal. Excepción: en Dibujo/Anime se prefiere un modelo x2 nativo si está en el manifiesto (más rápido y sin el paso extra).
- **8x** = **modelo x4 → reducción Lanczos3 a 0.5 → modelo x4**, con solape ampliado y gestión de tiles en cada pasada.
- **4x** = pasada única.

> **Corrección.** La versión inicial de este ADR decía "doble pasada del modelo x4" sin más. Eso da **16x**, no 8x: dos pasadas de un modelo x4 se multiplican. La reducción intermedia a la mitad es lo que hace que el producto sea `4 × 0.5 × 4 = 8`. Un test de `su-core` comprueba ahora que el producto de los factores de cada pipeline coincide con la escala que anuncia su identificador, precisamente para que este error no pueda repetirse.

**Justificación.** Alternativas descartadas: (a) recortar la salida del modelo x4 a la mitad pierde detalle real; (b) usar un modelo x2 dedicado para fotos no alcanza la calidad de los x4 actuales; (c) preescalar 2x con Lanczos y aplicar una sola pasada x4 es más rápido pero sintetiza menos detalle, y la prioridad del proyecto es la calidad. La reducción intermedia con Lanczos3 sobre una imagen x4 (que ya tiene detalle sintetizado) preserva mejor el detalle que escalar directamente con Lanczos.

**Consecuencias**

- (+) Un catálogo de modelos mucho más pequeño y coherente.
- (+) En 8x, la doble pasada supera a Upscayl (que hace una sola) en SSIM y LPIPS: es un criterio de aceptación verificable (AC-22).
- (−) 8x cuesta ~2.2× el tiempo de 4x (por el solape) y produce archivos muy grandes; la UI avisa con estimación de tiempo y tamaño antes de empezar.

---

## ADR-009 — Cola de trabajos persistente en SQLite

**Contexto.** Se requieren lotes grandes, reanudación de trabajos interrumpidos y resumen final con tiempos.

**Decisión.** SQLite (crate `rusqlite`, modo WAL) en `<appData>/jobs.db`, con checkpointing a nivel de tile en el sistema de archivos.

**Justificación.** Un lote de 1000 imágenes de 20 MP ×4 puede tardar horas. Sin persistencia, cerrar la app o un fallo del sidecar implica perder el trabajo. SQLite es embebido, sin servidor, transaccional y ya está disponible en todas las plataformas objetivo. Alternativas descartadas: JSON en disco (corrupción al escribir, sin consultas), LevelDB/Sled (peor para consultas relacionales de items).

**Consecuencias**

- (+) Reanudación real a tres niveles (lote, item, tile).
- (+) El historial de trabajos y las estadísticas de calibración viven en el mismo sitio.
- (−) Hay que gestionar migraciones de esquema (`user_version` + scripts versionados).
- (−) Los checkpoints de tiles consumen disco; un recolector limpia trabajos huérfanos > 7 días.

---

## ADR-010 — Telemetría opt-in, local-first

**Contexto.** El brief pide telemetría opt-in que registre errores, tiempos y uso de hardware.

**Decisión.** Interruptor apagado por defecto en el primer arranque, sin telemetría implícita. Las métricas se agregan localmente en `metrics.sqlite` y solo se envían si el usuario lo activa. Nunca se recogen rutas, nombres de archivo, imágenes ni identificadores estables del equipo.

**Justificación.** El público objetivo son profesionales que procesan material propio; la confianza es un requisito de producto, no un detalle legal. Separar "métricas locales" (siempre activas, útiles para el propio usuario y para el benchmark interno) de "envío remoto" (opt-in) permite tener buenos datos de diagnóstico sin comprometer la privacidad.

**Consecuencias**

- (+) Cumple RGPD por diseño (minimización y consentimiento explícito).
- (+) El informe de diagnóstico exportable hace triviales los reportes de bug.
- (−) La muestra remota será pequeña; las decisiones de producto se apoyarán más en el benchmark interno y menos en telemetría agregada.

---

## ADR-011 — Licencia: **MIT**

**Contexto.** El brief sugiere MIT.

**Decisión.** MIT para el código propio.

**Justificación.** Es la licencia más permisiva y compatible con el ecosistema (Electron, Next.js, Tailwind, Rust crates, ONNX Runtime son MIT/Apache-2.0). Aviso importante: **los modelos no son parte del repositorio ni del instalador**; se descargan bajo la licencia de su autor (Real-ESRGAN BSD-3-Clause, GFPGAN Apache-2.0, HAT Apache-2.0, CodeFormer S-Lab License 1.0 — esta última con restricciones de uso comercial). El gestor de modelos muestra la licencia de cada modelo antes de descargarlo y el README lo advierte explícitamente.

**Consecuencias**

- (+) Máxima adopción y compatibilidad.
- (−) No se puede relicenciar a GPL/Apache más adelante sin permiso de todos los contribuidores; aceptable dado el objetivo.
- (−) La licencia de CodeFormer debe quedar clara para usuarios comerciales; se marca como "uso comercial restringido" en la UI.

---

## ADR-012 — Distribución: instalador base sin CUDA/TensorRT + "Acceleration Packs"

**Contexto.** Los binarios de ONNX Runtime con CUDA/TensorRT pesan cientos de MB (más las DLLs de cuDNN/TensorRT). Incluirlos siempre hace que un usuario con AMD o con CPU descargue medio giga inútil.

**Decisión.** El instalador base incluye: sidecar + ORT CPU + ORT DirectML (solo Windows) + ORT CoreML (solo macOS) — es decir, todas las rutas sin dependencias propietarias pesadas. Los EPs NVIDIA (CUDA y TensorRT) se ofrecen como **Acceleration Pack** descargable (con hash verificado), detectado en el primer arranque: si hay GPU NVIDIA, la app propone instalarlo.

**Justificación.** Optimiza la descarga para el caso más común sin penalizar al usuario de NVIDIA. El coste es un paso extra en la primera ejecución, que se compensa con una estimación de tiempo y de ganancia ("~4–8× más rápido en tu RTX 3060").

**Consecuencias**

- (+) Instalador base ~120 MB en lugar de ~700 MB.
- (+) Se puede actualizar el pack de aceleración (nueva versión de TensorRT) sin publicar una versión de la app.
- (−) Requiere un mecanismo de descarga y verificación (que ya existe para los modelos, se reutiliza).
- (−) Hay que manejar el caso "pack instalado pero driver demasiado antiguo" con un mensaje claro.

---

## ADR-013 — Aplicar EXIF de orientación antes de inferir

**Contexto.** Muchas fotos de móvil llevan la orientación en EXIF y los píxeles sin rotar.

**Decisión.** Normalizar la orientación (y eliminar el tag de orientación) inmediatamente después de decodificar, antes de cualquier análisis o inferencia.

**Justificación.** Si no se hace: (a) la detección de rostros falla en imágenes rotadas 90°, (b) el resultado sale girado respecto al original, (c) la restauración facial se aplica sobre caras tumbadas. Es una fuente clásica de bugs silenciosos en herramientas de este tipo.

**Consecuencias**

- (+) El resultado siempre tiene la orientación visual correcta.
- (−) Un ciclo extra de copia en memoria en el caso poco común de orientación ≠ 1; se evita aplicando la transformación en el propio decodificador cuando es posible.

---

## ADR-014 — Un worker de inferencia por GPU (y por qué no "2 hilos" en CPU)

**Contexto.** Upscayl expone un ajuste de "2 hilos" y en CPU suele limitar el paralelismo, lo que deja núcleos ociosos.

**Decisión.** En GPU: **1 sesión de inferencia activa por GPU** (configurable, pero con 1 por defecto). En CPU: **1 sesión** con `intra_op_num_threads = núcleos físicos` y `inter_op_num_threads = 1`, más workers paralelos de E/S.

**Justificación.** Dos sesiones concurrentes en la misma GPU no aumentan el throughput (el dispositivo ya está saturado de trabajo) y duplican el pico de VRAM, justo lo que queremos evitar. En CPU, varias sesiones de ORT compiten por el mismo ancho de banda de memoria y el rendimiento agregado suele *empeorar*; el paralelismo útil está en decodificar/codificar en paralelo con la inferencia, que es donde realmente se solapan los cuellos de botella.

**Consecuencias**

- (+) Pico de VRAM predecible y mínimo.
- (+) En CPU se usan todos los núcleos físicos de verdad (requisito AC-12).
- (−) El ajuste "concurrencia" en Configuración avanzada solo tiene efecto real con varias GPUs o en el pool de E/S; la UI lo explica con un tooltip.

---

## ADR-015 — Escritura atómica y validación de salida obligatoria

**Contexto.** El brief exige que nunca se generen imágenes negras o corruptas sin avisar.

**Decisión.** Ningún archivo de salida se escribe directamente. El flujo es: componer en memoria → validar (dimensiones, desviación estándar, ausencia de NaN/Inf, rango dinámico) → escribir en `<destino>.su-tmp` → `fsync` → `rename` atómico. Si la validación falla, no se escribe nada y el item se marca fallido con código `SU-E141`, conservando los tiles crudos para diagnóstico.

**Justificación.** Un `rename` en el mismo sistema de archivos es atómico en NTFS, APFS y ext4. Esto garantiza que una interrupción (apagón, kill -9, disco lleno) no deje un archivo a medias que el usuario crea válido. La validación de contenido detecta el fallo más insidioso: un modelo mal convertido o un EP que devuelve un buffer vacío sin lanzar error.

**Consecuencias**

- (+) Nunca hay archivos corruptos en la carpeta de salida.
- (+) Coste: una pasada de estadísticas sobre la imagen de salida (~20 ms por 8 MP); despreciable frente a la inferencia.
- (−) Se requiere espacio temporal en el mismo volumen que el destino; si el disco está lleno, el error es claro (`SU-E150`) en lugar de un archivo truncado.

---

## ADR-016 — npm workspaces en lugar de pnpm

**Contexto.** El plan inicial del monorepo asumia pnpm. Al preparar la Fase 1 se comprobo que la maquina de desarrollo no tiene `pnpm` instalado ni `corepack` activado, mientras que `npm` 10.9.7 (con soporte nativo de workspaces) si esta disponible.

**Opciones evaluadas**

| Criterio | npm workspaces | pnpm | yarn berry |
|---|---|---|---|
| Disponible ya en el equipo | Si | Requiere instalacion | Requiere instalacion |
| Enlazado de paquetes del monorepo | Automatico | Automatico | Automatico |
| Eficiencia de disco | node_modules plano | Store con hardlinks | PnP / node_modules |
| Estrictez de dependencias | Permisiva (hoisting) | Estricta (sin hoisting) | Estricta |
| Electron + binarios nativos | Sin fricción | Sin fricción | PnP da problemas conocidos con Electron |

**Decision.** **npm workspaces**, con la puerta abierta a migrar a pnpm mas adelante (el `package.json` de cada workspace ya declara sus dependencias correctamente, que es lo unico que hace falta para que la migracion sea `pnpm import` + `pnpm install`).

**Justificacion.** Para un monorepo de cuatro paquetes, la estrictez de pnpm no compensa el coste de anadir un gestor de paquetes a la cadena de herramientas de un proyecto que debe construirse en Windows, macOS y Linux. npm ya viene con Node, y en un proyecto de escritorio con binarios nativos multiplataforma, cuantas menos piezas moviles mejor.

**Consecuencias**

- (+) Cero dependencias de tooling adicionales: `npm install` y a funcionar.
- (+) El CI solo necesita Node.
- (−) El hoisting de npm permite importar paquetes que no estan declarados. Se mitiga con `typecheck` por workspace y, en la Fase 5, con una comprobacion de dependencias no declaradas.
- (−) Menos eficiente en disco que el store de pnpm.

---

## ADR-017 — Sin libreria de esquemas ni i18n en la Fase 1

**Contexto.** El plan inicial preveia `zod` para validar la frontera IPC e `i18next` para las traducciones.

**Decision.**

1. **Validacion a mano** (`packages/shared/src/guards.ts`) en lugar de zod. La superficie del preload en la Fase 1 son ocho metodos con payloads triviales (listas de rutas, parches de ajustes con campos conocidos). Se escriben validadores explicitos que devuelven objetos limpios y descartan lo invalido en silencio, que es exactamente el comportamiento que se quiere en esa frontera.
2. **Diccionario tipado propio** (`packages/shared/src/i18n`) en lugar de i18next. Con dos idiomas y una app de escritorio sin carga remota, i18next aporta deteccion de idioma, plurales y carga asincrona de recursos que aqui no se usan. El diccionario propio da lo que si importa: claves tipadas (una clave que falta en `en.ts` es un error de compilacion) y cero dependencias.

**Justificacion.** Ambas decisiones son coherentes con el objetivo de distribucion: cada dependencia en el renderer acaba dentro del instalador y hay que versionarla y auditarla en tres plataformas. Se anaden cuando aporten algo que no se pueda escribir en cincuenta lineas.

**Consecuencias**

- (+) Dos dependencias menos en el renderer; sin superficie de CVE que seguir.
- (+) Traducciones tipadas de extremo a extremo: `t('queue.empty')` no compila si la clave no existe.
- (+) Los validadores devuelven datos normalizados, no lanzan: la UI sigue siendo usable con un cliente desactualizado.
- (−) Si la API del sidecar crece (Fase 4), los esquemas a mano se vuelven insostenibles. En ese momento se generaran desde el OpenAPI con `openapi-typescript` + `zod`, que es cuando zod aporta valor real (esquemas derivados de una fuente unica, no escritos a mano).
- (−) El diccionario propio no soporta plurales ni interpolacion avanzada. La interpolacion `{count}` ya esta implementada; los plurales se resolveran con claves explicitas (`_one` / `_many`) si llegan a hacer falta.

---

## ADR-018 — ONNX Runtime detrás de una feature opcional, con una abstracción `Backend`

**Contexto.** El plan inicial ponía ONNX Runtime en el centro del sidecar. En la práctica eso significa que **nada** del sidecar se puede compilar ni testear sin descargar cientos de megabytes de bibliotecas nativas, sin un modelo y, en muchos equipos, sin GPU.

**Decisión.** La inferencia pasa por el trait `Backend` (`su-inference::backend`). La implementación con ONNX Runtime (`su-inference::ort_backend`) vive detrás de la feature `onnx`, **desactivada por defecto**. Se incluye un `MockBackend` que reescala con la geometría exacta del modelo al que sustituye.

> **Nota sobre el camino.** La feature se declaró en un primer momento con las dependencias opcionales de ORT pero sin implementación detrás, y se retiró: prometía una capacidad que no existía y quien la activara se habría encontrado con un error de módulo ausente en lugar de con un mensaje que explicara qué pasaba. Volvió junto con `ort_backend.rs`, que es cuando empezó a significar algo.
>
> También se decidió **no activar `download-binaries`** de la crate `ort`: ONNX Runtime lo aporta la aplicación, no el crate. Sin eso, `cargo build --features onnx` intentaría descargar cientos de megabytes, y el diseño de "Acceleration Packs" (ADR-012) perdería su sentido.

**Consecuencias adicionales de `load-dynamic`**

- (+) El binario del sidecar no crece con ORT. Un usuario con AMD no descarga las bibliotecas de NVIDIA.
- (+) Se puede actualizar TensorRT sin publicar una versión de la app.
- (−) Hay que distribuir el binario de ONNX Runtime aparte y verificar su hash, con el mismo mecanismo que los modelos.
- (−) Sin `ndarray` (no se activa): la entrada se construye con `(shape, Vec<f32>)` y la salida se lee con `try_extract_tensor`, que devuelve `&[f32]`. Para pasar tiles de imagen no hace falta nada más, y es una dependencia menos.

**Justificación.** El tiling, la composición sin costuras, la degradación por falta de memoria, la evaluación de condiciones y el encadenado de etapas son **donde están los errores que importan**, y ninguna de esas cosas necesita un modelo. Con la abstracción por delante:

- 307 tests del workspace se ejecutan sin ORT, sin GPU y sin descargar un solo modelo.
- `su-cli upscale` funciona recién clonado el repositorio, lo que hace que el flujo completo sea verificable en CI.
- `MockBackend` es además el **patrón de referencia**: si al trocear aparece una costura con un backend que solo replica píxeles, el fallo está en la composición, no en el modelo. Depurar eso con un modelo real sería mucho más difícil.
- Cambiar de runtime (ONNX, TensorRT nativo, lo que venga) no toca ni el motor de pipelines ni la planificación de tiles.

**Consecuencias**

- (+) El sidecar se compila y se testea en segundos, no en minutos, y sin hardware especial.
- (+) `cargo test` es una verificación real del 90 % del código, no un simulacro.
- (+) Añadir otro runtime es implementar un trait, no reescribir el pipeline.
- (−) Hay una indirección más en la ruta caliente. Es un salto virtual por tile, despreciable frente al coste de una inferencia.
- (−) El `MockBackend` **no sintetiza detalle**: sirve para verificar geometría y composición, no calidad. Los criterios de aceptación de calidad (AC-20 a AC-22) requieren un modelo real y por eso pertenecen a la Fase 5.
- (−) Hay que mantener el contrato del trait cuando se añada un modelo con requisitos nuevos (por ejemplo, entrada en `uint8`). El trait documenta que la entrada y la salida son `f32` entrelazado en `0..1`, precisamente para que eso no ocurra sin darse cuenta.

---

## ADR-019 — La descarga de modelos vive en la aplicación, no en el sidecar

**Contexto.** El manifiesto de modelos declara de dónde traer cada archivo (`urls`), su `sha256` y su tamaño. Alguien tiene que hacer la descarga. Las dos opciones eran el sidecar (Rust) o el proceso principal (Electron/Node).

**Decisión.** La descarga se hace en el **proceso principal**, en `apps/desktop/src/main/models/downloader.ts`. El sidecar **describe** (publica `ModelStatus.download` con URL, hash y tamaño, y expone `modelsDir` en `/v1/models`); la aplicación **trae el archivo** y lo deja en el directorio que el sidecar le indique.

**Motivos, por orden de peso:**

1. **Un cliente HTTP en Rust arrastra TLS, y TLS arrastra código C.** `rustls` necesita `ring` o `aws-lc-rs`, que compilan C y ensamblador. Eso obliga a un compilador de C en las tres plataformas para una función que no lo necesita, y complica el `cargo test` sin GPU ni modelos que sostiene toda la verificación del sidecar (ADR-018).
2. **El proxy del sistema es cosa de la aplicación.** Electron lo gestiona por configuración; en Rust habría que implementarlo.
3. **El progreso y la cancelación son de la interfaz.** El canal IPC ya existe; en Rust habría que añadir un evento nuevo al protocolo del sidecar.
4. **El directorio de datos del usuario lo conoce la aplicación.** Node trae `fetch`, `crypto` y streaming en la biblioteca estándar: cero dependencias nuevas.

**Consecuencias**

- (+) El sidecar sigue compilando sin TLS y sin compilador de C, y sus tests siguen corriendo sin red.
- (+) El descargador es **puro**: solo importa `node:*`, así que se prueba de verdad con `node --test` contra un servidor HTTP local. Son 15 tests que cubren reanudación, hash incorrecto, espejos caídos, cancelación y conexiones mudas.
- (+) El `sha256` se comprueba **dos veces y en dos sitios**: al terminar la descarga (en Node) y al cargar el modelo (en Rust, `SU-E111`). El segundo es el que manda; el primero solo evita guardar basura.
- (−) Hay una verificación duplicada de `sha256`. Se acepta a cambio de que la descarga funcione: `sha256` es `sha256` en los dos lenguajes.
- (−) **`modelsDir` no se puede calcular en la interfaz.** Es el sidecar quien resuelve la ruta por plataforma y la publica; si la interfaz la dedujera por su cuenta, las dos podrían separarse y la lista diría «instalado» mientras el motor sigue sin encontrar el archivo.
- (−) La descarga depende de que el sidecar esté en pie para saber dónde escribir. Si el motor no arranca, no se pueden descargar modelos.

**El mismo criterio aplica a los ZIP y CBZ.** La interfaz anuncia ZIP y CBZ como formato de entrada, y un CBZ es un ZIP con imágenes dentro. La expansión vive en la aplicación (`apps/desktop/src/main/archives/`) por el mismo motivo que la descarga: es una tarea de sistema de archivos que la aplicación resuelve sin dependencias nuevas (`node:zlib` ya trae el inflate y el CRC), y así el fallo aparece **antes** de encolar el trabajo en lugar de a mitad del lote. El sidecar sigue recibiendo rutas de imagen, que es lo que sabe procesar.

Un ZIP se lee por su **directorio central**, no por sus cabeceras locales: un ZIP escrito en streaming lleva los tamaños a cero en la cabecera local y los valores reales en un descriptor detrás de los datos. Además, cada entrada se comprueba contra su CRC, porque un byte corrupto que aun así se descomprime saldría como una imagen con basura en lugar de como un error. ZIP64, las entradas cifradas y la compresión que no sea `stored` o `deflate` se **detectan y se rechazan con su motivo**, en vez de intentar leerlos a medias.

**Nota sobre la firma.** El manifiesto sigue especificando una firma Ed25519 que **no está implementada**. Hasta que lo esté, la integridad de un modelo descargado se apoya en el `sha256` que declara el propio manifiesto, y el manifiesto viaja con la aplicación. Eso protege contra un espejo comprometido, no contra un manifiesto manipulado.

---

## ADR-020 — El proveedor de backends es la única fuente de verdad sobre el backend

**Contexto.** `Capabilities` describe la máquina: qué GPUs hay, cuánta VRAM libre, qué execution provider se recomienda. `BackendProvider` describe lo que se va a ejecutar de verdad. Son cosas distintas en cuanto se compila sin la feature `onnx`, y sin embargo el informe del trabajo y las variables `hardware.isCpu` / `hardware.freeVramMb` se calculaban a partir de `Capabilities`.

El resultado era un informe que mentía. En un equipo con TensorRT, `su-cli` anunciaba `TensorRT` en el resultado aunque el backend fuera el de referencia, que corre en CPU. Peor: `hardware.isCpu` es una variable de `EvalVars`, así que una condición de pipeline como `hardware.isCpu == false` elegía la rama de GPU y el motor ejecutaba algo distinto de lo que la condición creía.

**Decisión.** El trait `BackendProvider` expone dos datos más, con implementación por defecto:

- `name()` → el proveedor que se va a usar, para el informe. Por defecto `"referencia"`.
- `uses_vram()` → si consume VRAM. Por defecto `false`.

`OrtProvider` los sobrescribe con el EP configurado (`self.provider.as_str()` y `self.provider.uses_vram()`). `su-cli` y `su-server` derivan `provider_name`, `is_cpu` y `free_vram_mb` del proveedor, no de `Capabilities`. Además, `su-cli` declara `onnx = ["su-inference/onnx"]`: sin ese puente la feature no se podía activar desde el binario y `OrtProvider` quedaba inalcanzable, que es lo que hacía que el backend real no se usara nunca.

**Consecuencias**

- (+) El informe dice el backend que se ejecutó. Ya no puede haber discrepancia entre lo que se anuncia y lo que se ejecuta, porque el dato sale del mismo sitio que el backend.
- (+) Las condiciones de pipeline que dependen del hardware ven la verdad: sin `onnx`, `isCpu` es `true` y no se presupuesta VRAM.
- (+) `cargo build -p su-cli --features onnx`, que la documentación prometía desde el principio, ahora funciona y sirve para algo.
- (+) Los tests fijan el contrato: hay un test por cada configuración de compilación que comprueba el nombre y la VRAM declarados.
- (−) Dos métodos más en el trait. Se mitiga con implementación por defecto, para que los proveedores de los tests no tengan que implementarlos.
- (−) `Capabilities` sigue siendo la fuente de la VRAM **libre** (`best_vram_free_mb`), que es hardware y no compilación. El reparto es: el proveedor dice *si* se usa VRAM, `Capabilities` dice *cuánta* hay.
- (−) Sigue sin poder elegirse la GPU por índice desde el CLI: el dispositivo va fijo a 0.

**Nota.** El backend de ONNX Runtime compila y sus pruebas pasan (`cargo test --workspace --all-features`, ver ADR-022), pero **no se ha ejecutado nunca con un modelo real**: eso requiere el binario de ONNX Runtime y un modelo descargado. Es el paso que queda pendiente de verificación.

---

## ADR-021 — Cuatro ajustes de la interfaz que el motor ignoraba

**Contexto.** Una auditoría de `JobOptions`/`JobRequest` contra quién los **lee** de verdad encontró cuatro campos que viajaban desde la interfaz hasta el motor y morían ahí: `device`, `concurrency`, `unload_between_images` y `priority`. `DeviceChoice` solo aparecía en su propia definición y en el valor por defecto; `concurrency` en un `assert` de un test; `unload_between_images` en ninguna parte, con `OrtProvider::unload_all()` escrito y sin llamar desde ningún sitio; `priority` en un fixture.

El efecto era el peor posible: el usuario cambiaba un control, no pasaba nada, y la aplicación no decía nada. Y tres de esos ajustes estaban **documentados en `docs/05` como funciones reales**, así que la documentación también mentía.

**Decisión.** Se conectan los cuatro, cada uno en el sitio que le corresponde.

**`device` — el proveedor se elige por trabajo.** `AppState` recibe un `Providers { default, cpu }`: el que corresponde al hardware y, si el primero usa VRAM, uno forzado a `ProviderKind::Cpu`. `JobContext` sale del que pida el trabajo. No se añade un método al trait `BackendProvider` para derivar variantes porque exigiría un receptor `Arc<Self>` y subir el MSRV declarado (`1.75`); construir dos proveedores donde ya se construía uno es más simple y no cambia el contrato público. Pedir GPU en un equipo sin GPU **no es un error**: se usa el de por defecto y el informe del trabajo publica el proveedor real, así que la interfaz dice lo que pasó.

**`concurrency` y `priority` — una cola de verdad.** `JobManager` deja de lanzar un hilo por trabajo al crearlo. Los trabajos creados entran en una cola y `pump()` arranca los que quepan: más prioridad primero y, a igualdad, el que llegó antes. El límite es **global**, no por trabajo, porque es como lo ve el usuario (una preferencia del equipo); el valor llega en las opciones del trabajo, así que el último creado lo fija. La reanudación pasa por la misma cola, en lugar de arrancar un hilo suelto que se saltaría el límite.

**`unload_between_images` — un método en el trait.** `BackendProvider` gana `unload()`, con implementación vacía por defecto; `OrtProvider` la sobrescribe con `unload_all()`. `su-jobs` la llama en la frontera entre imágenes, que es donde el ajuste promete el ahorro.

**Telemetría.** `su-cli` llama a `su_telemetry::init()` al arrancar, con el directorio de datos del usuario y su ruta personal enmascarada. Como el subscriber de `tracing` es global al proceso, con eso basta para que las trazas de todos los crates —incluido `su-server`— acaben en el archivo; por eso se **quita** `su-telemetry` de las dependencias de `su-server`, donde estaba declarada sin usarse.

**Consecuencias**

- (+) Los cuatro controles hacen lo que dicen. Tres de ellos ya lo decían en `docs/05`, así que ahora la documentación es cierta.
- (+) El límite de concurrencia protege de verdad la VRAM: dos inferencias simultáneas en la misma GPU se estorban más de lo que se ayudan.
- (+) Hay 11 pruebas de integración que levantan el servidor real en un puerto efímero y hablan por socket, así que el contrato con la aplicación —incluido el **fichero de puerto** que Electron lee— deja de depender de que nadie lo rompa sin enterarse.
- (−) El turno se devuelve con un `Drop` (`SlotGuard`) y no con una llamada al final del bucle. Es más indirecto, pero un `panic` o una salida temprana con el contador desajustado dejarían la cola **parada para siempre**.
- (−) El límite de concurrencia es global y el último trabajo creado lo fija. Es una lectura sorprendente de un ajuste por trabajo, y se acepta a cambio de que el control signifique algo; queda documentado en el propio método.
- (−) Un trabajo reanudado entra con prioridad 0, porque `Job` no guarda la prioridad de la petición original. Reanudar es una acción del usuario sobre un trabajo concreto, así que competir por prioridad no aporta.
- (−) `set_max_concurrent` y `running()` son API pública que existe sobre todo para poder probar el planificador. Se acepta: además sirven para diagnóstico.

---

## ADR-022 — Lo que apareció al ejecutar por primera vez las pruebas del sidecar

**Contexto.** El sidecar se escribió en un entorno donde solo se llegó a `cargo check` (ver ADR-018 y el punto 1 de las tareas pendientes de `docs/01`). Al ejecutar `cargo test` por primera vez en un equipo con Rust completo, **10 de las 287 pruebas de entonces fallaron**. No era ruido: siete de los fallos eran defectos de producción y cuatro, pruebas que afirmaban algo distinto de lo que el código hace bien.

**Decisión.** Se corrige cada uno en el lado que le corresponde —el código cuando el código estaba mal, la prueba cuando la prueba estaba mal— y se deja constancia de los dos criterios que salen de aquí.

### Defectos de producción

1. **Identificadores de trabajo que colisionaban.** El número de creación vivía en cada `JobManager`. Dos gestores creados en el mismo milisegundo generaban el mismo `job-<ms>-0001`, así que un trabajo nuevo podía sobrescribir el registro de otro en SQLite. Ahora el contador es **global al proceso** (`next_sequence`, con `AtomicU64`), lo que además resuelve sin candado la carrera entre hilos que crean trabajos a la vez. La marca de tiempo sigue cubriendo el caso entre procesos.
2. **Orden del listado dependiente del azar.** `created_at` tiene precisión de segundo, así que los trabajos de un mismo lote empataban y el desempate quedaba en manos del recorrido de un `HashMap`. Se desempata por identificador, que lleva milisegundos y número de creación.
3. **Los trabajos interrumpidos volvían como `Running`.** `restore()` leía la lista **antes** de llamar a `take_interrupted()` —que es quien pasa los trabajos a `Paused` y lo persiste—, así que las copias en memoria conservaban el estado antiguo. La interfaz mostraba un trabajo "en curso" que no avanzaba: exactamente lo que la reanudación existe para evitar. Ahora se lee después.
4. **Dos trabajos a la vez escribiendo la misma salida.** El temporal de escritura atómica se llamaba `<destino>.su-tmp`. Al subir el límite de concurrencia, dos imágenes idénticas del mismo lote apuntan al mismo destino: la segunda escritura pisaba el temporal de la primera y el trabajo terminaba en fallo sin que nada estuviera roto. El temporal pasa a llevar pid y contador. Esto **matiza ADR-015**: la escritura sigue siendo atómica, pero ya no da por supuesto que hay un solo escritor.
5. **El modelo manual se cargaba y se tiraba.** El modelo del pipeline se abría antes de comprobar el `modelOverride`, para leer su escala y decidir después. En la máquina, eso es abrir una sesión de ONNX Runtime —y reservar su VRAM— para descartarla acto seguido; en un modelo de 67 MB, además, es tiempo. Se añade `BackendProvider::native_scale()`, con implementación por defecto `None`, para poder decidir **antes** de cargar. `OrtProvider` la responde desde el manifiesto, que ya declara la escala. Cuando el proveedor no la sabe, la etapa no se sustituye: es preferible no aplicar el ajuste a escalar con un modelo de restauración.
6. **El token nunca llegaba al sidecar.** `docs/02` y el propio supervisor documentan que el token viaja en `SU_TOKEN` "nunca como argumento". El supervisor lo ponía en el entorno, pero `su-cli` solo leía `--token`, así que generaba un token aleatorio distinto del que usa la aplicación. Resultado: **401 en todas las peticiones autenticadas**, el flujo de eventos por WebSocket nunca conectaba y la interfaz se quedaba sin progreso. El CLI ahora lee `SU_TOKEN`, y en el orden argumento → entorno → generado. Un valor vacío **no** cuenta como token: aceptarlo dejaría la API de loopback abierta a cualquier proceso del equipo, y una variable mal definida no puede degradar la seguridad en silencio.
7. **El manifiesto aceptaba una escala imposible.** `validate()` admitía cualquier escala de 1 a 8, así que un modelo con escala 3 pasaba la validación. Con el catálogo cerrado a 1, 2, 4 y 8 (más el 1 de los modelos que no escalan), un 3 no es un valor raro: es un error que aparecería mucho más tarde, al escribir una imagen del tamaño equivocado.

### Pruebas equivocadas

1. **`200` en lugar de `201`.** `POST /v1/jobs` crea un recurso y responde `201`, que es lo correcto. El cliente comprueba `response.ok`, así que ninguna parte de la aplicación dependía del `200`.
2. **Un ancho de imagen escrito a mano.** La prueba de geometría con tiling indexaba la salida con la constante `800` de otra prueba, sobre una imagen de 300 px que a 4x mide 1200. Leía otra fila y acusaba al tiling de un desplazamiento que no existía. Ahora usa el ancho real y comprueba además las dimensiones.
3. **Un sondeo fuera del alcance del enforcado.** La prueba de `unsharp` medía el cambio a 8 y 22 píxeles del escalón, y el desenfoque es una caja de radio 1 aplicada tres veces: solo alcanza tres píxeles a cada lado. Que ahí no cambie nada es el comportamiento correcto —el enfoque no debe tocar las zonas planas—, así que la prueba comprobaba lo contrario de lo que quería. Sondea pegado al escalón.
4. **Un `sha256` de seis caracteres.** El código exige al menos ocho para aceptar un valor como hash, y hace bien: un literal de seis no es un hash. La prueba usaba uno corto por comodidad y esperaba un nombre de archivo que el código nunca generaría. El fixture pasa a usar 64 hexadecimales, como el resto de las pruebas del mismo archivo.

**Consecuencias**

- (+) `cargo test --workspace --all-features` pasa con **326 pruebas** (307 sin la feature `onnx`), y el workspace compila sin un solo aviso.
- (+) La aplicación arranca en Linux, el sidecar responde y el flujo de eventos conecta. Verificado con Electron 44 y el binario real.
- (+) Queda fijado el criterio: **una prueba que afirma algo distinto de lo que el código hace bien es un fallo de la prueba**, y se corrige ahí; pero antes de tocar la prueba hay que comprobar que el código no es el que está mal. Tres de los siete defectos de producción se veían precisamente en pruebas que parecían equivocadas.
- (−) Sigue **sin ejecutarse una inferencia real**: hace falta el binario de ONNX Runtime y un modelo descargado. Todo lo anterior es la verificación de todo lo que se puede verificar sin ellos.
- (−) Los siete defectos compartían una causa: nunca se ejecutó nada. La lección operativa es que el plan de pruebas de `docs/07` tiene que ejecutarse **en el entorno de desarrollo**, y si el entorno no lo permite, eso forma parte del trabajo y no una nota al pie.

---

## ADR-023 — La ventana que se veía pero no respondía

**Contexto.** La aplicación construida mostraba la interfaz completa —tema morado, barra lateral, modos, escala, botones— y **ningún control hacía nada**: ni cambiar de modo, ni abrir el selector de imágenes, ni desplegar las opciones avanzadas. No había ningún error visible ni ninguna línea en el registro. Solo ocurría en **producción**: con `npm run dev` la interfaz funcionaba, que es exactamente por lo que el defecto sobrevivió a toda la fase de desarrollo.

**Causa.** El export estático de Next no arranca solo con scripts externos: incluye dos `<script>` **sin `src`**, el que crea `self.__next_f` y el que empuja los datos del árbol de React. La CSP del esquema `app://` era `script-src 'self'`, sin `'unsafe-inline'`, sin `nonce` y sin hashes. Chromium bloqueaba esos dos scripts, y sin ellos no hay bootstrap: React nunca hidrataba.

Lo que hacía el fallo tan difícil de leer es su forma. El HTML prerenderizado y el CSS **son** estáticos: la página se pinta entera y con su estilo. Lo único que falta es el JavaScript, así que no se ve una pantalla en blanco sino una aplicación terminada que ignora el ratón. En desarrollo no aparece porque el renderer se sirve por HTTP, donde esta cabecera no se aplica.

**Decisión.** Los hashes de los scripts en línea se calculan en el proceso principal y se añaden a `script-src`. Se elige el hash y no `'unsafe-inline'` porque la CSP existe para que `script-src 'self'` signifique algo, y `'unsafe-inline'` la dejaría sin efecto sobre scripts: sería convertir una defensa real en un párrafo decorativo. Los hashes se calculan sobre **el mismo texto que se sirve**, nunca sobre una lectura aparte del archivo, porque un hash que deja de coincidir por un salto de línea normalizado vuelve a dejar la ventana muda.

Además, la ventana **deja de ser muda**: se reenvían al registro los errores y avisos de la consola del renderer, los fallos del preload y los fallos de carga del marco principal. Antes, un script bloqueado por la CSP o un preload roto no dejaban rastro alguno en ninguna parte.

**Consecuencias**

- (+) La interfaz responde. Verificado contra la ventana real de Electron, alternando el selector de modo en los dos sentidos, no deduciéndolo del código.
- (+) Los hashes calculados coinciden **exactamente** con los que el propio Chromium pide en el mensaje de violación, que es la comprobación más fuerte posible: la hizo el navegador, no el proyecto.
- (+) `script-src` sigue siendo `'self'` para los scripts externos, que son la mayoría. Solo se autoriza, por su hash, el contenido exacto de los scripts en línea del documento servido.
- (+) Un fallo de este tipo ya no puede ser invisible: aparece en el registro como `renderer.console-error` con el texto de la violación, que además incluye los hashes que faltan.
- (−) La cabecera CSP se calcula por documento (leer el HTML, buscar los scripts en línea y hashear). Son decenas de KB por navegación: irrelevante para una ventana que se carga una vez, y es el precio de que la cabecera no pueda desincronizarse del documento.
- (−) Un cambio futuro de Next que mueva la hidratación a otra forma (por ejemplo, totalmente externa) haría que los hashes sobraran, no que faltaran: el fallo por defecto pasa a ser inofensivo. Al revés —un `nonce` fijo— volvería a fallar en silencio.
- (−) **Ninguna prueba automática cubre esto.** Los tests de Node no pueden cargar una página en un navegador, así que la comprobación de que la ventana responde se añade al plan de pruebas manuales de `docs/07` como paso obligatorio de cada entrega.

---

## ADR-024 — El trabajo se aceptaba, pero no se podía seguir; y la interfaz decía que simulaba

**Contexto.** El síntoma que reportó el usuario fue «cuando quiero upscalear una imagen me sale error», y el registro terminaba con `Uncaught (in promise) Error: Error invoking remote method 'su:app:reveal': reply was never sent`. Ese mensaje no era la causa sino la cola: describía un manejador de IPC que nunca contestó.

Al reproducir el flujo completo con la ventana real aparecieron **tres defectos de producción** y un cuarto de otra familia.

1. **Toda la API por identificador estaba muerta.** El proyecto fija `axum` en **0.7.9**, donde un parámetro de ruta se escribe `:id`. Las rutas estaban escritas con `{id}`, que es la sintaxis de axum 0.8: para el enrutador de 0.7 eso no es un comodín sino el texto literal `{id}`. Consecuencia: `GET`, `pause`, `resume` y `cancel` sobre un trabajo concreto respondían **404 para cualquier identificador**. Nada de esto se veía en las pruebas porque la que cubría esas rutas comprobaba que la respuesta no fuera un 500, y un 404 la satisface: **pasaba por el motivo equivocado**.

2. **`shell.openPath` sobre una carpeta nunca resuelve en Linux.** No devuelve cuando el gestor de archivos *arranca*, sino cuando **termina**, y un gestor de archivos no termina. Medido en este equipo: abrir `/tmp` no resolvió en 60 segundos. Como la llamada vive dentro de un manejador de IPC, la interfaz se quedaba con una promesa que nunca contesta y Electron respondía `reply was never sent`. Se llega a esto desde tres sitios visibles: abrir la carpeta de salida, abrir la carpeta de modelos y la acción «exportar diagnóstico» de un aviso.

3. **Pausar un trabajo ya terminado era un error interno.** Los controles se eliminan al terminar, así que el manejador no encontraba nada y devolvía 500. Es alcanzable desde la interfaz: basta pulsar «Pausar» justo cuando el lote acaba.

4. **La interfaz decía cosas que ya no eran ciertas.** `mockBackend` estaba fijado a `true` en el proceso principal, y sobre ese campo se mostraban tres textos: la insignia «Fase 1 · interfaz sin backend» en la barra lateral, el aviso «el progreso es una simulación» en las opciones avanzadas y, el peor, la etiqueta **«Progreso simulado» junto a la barra de progreso de un trabajo real**. El proyecto prohíbe las simulaciones silenciosas en su propia documentación; esto era lo contrario y también peor: una simulación anunciada donde no la había.

**Decisión.**

- Las rutas se escriben con la sintaxis que corresponde a la versión fijada (`:id`), y la prueba deja de conformarse con «no es 500»: comprueba que la ruta **llega al manejador**, distinguiendo el 404 de «esa ruta no existe» del 405 de «ese método no vale aquí». Una prueba que no distingue esos dos casos no prueba el enrutador.
- Abrir una carpeta espera con **límite** (1,5 s). Que el límite venza significa que el sistema lanzó algo que sigue vivo, que es el caso normal, y queda anotado en el registro como `shell.open-path-sin-respuesta`. Un fallo de verdad —una carpeta que no existe— llega antes y se reporta como tal. Los archivos siguen usando `shell.showItemInFolder`, que es síncrono.
- Controlar un trabajo que ya terminó no es un fallo del motor: se contesta con el estado real en lugar de con un 500. Un 500 dice «el motor está roto» y aquí no lo estaba.
- El aviso de motor ausente se deriva del estado **vivo** (`sidecarReadyAtom`), no de un campo fijado al arrancar: el sidecar tarda en estar listo y un dato congelado mentiría justo durante los primeros segundos, que es cuando el usuario mira. Se elimina `mockBackend` de `AppInfo`, se quitan la insignia y la etiqueta de progreso simulado, y la clave `phase1` pasa a llamarse `noBridge`, que es lo que de verdad describe: la página se abrió fuera de Electron.

**Consecuencias**

- (+) Verificado conduciendo la ventana real de Electron: se arrastra y suelta una imagen, se pulsa «Upscaly» con un clic de ratón de verdad y el trabajo recorre sus etapas hasta `Completado`, con el PNG de salida en disco a la escala pedida (48×32 → 192×128). **Cero errores en la página** en toda la operación.
- (+) La comprobación de que el trabajo se puede *seguir*, no solo crear, es la que faltaba: el defecto 1 no rompía la creación, así que todas las pruebas anteriores pasaban.
- (+) Abrir carpeta responde en 1,5 s en lugar de no responder nunca; un archivo, en 2 ms; una ruta inexistente devuelve `false` en lugar de un error opaco.
- (−) El límite de 1,5 s es una espera optimista: un gestor de archivos que tardara más se registraría como «sin respuesta» aunque acabara abriéndose. Se prefiere eso a reportar un fallo falso, que es lo que haría un `Promise.race` con resultado negativo.
- (−) La etiqueta de progreso se elimina en lugar de corregirse: cualquier texto que prometa algo sobre el motor sin consultarlo acabará mintiendo, y el estado del motor ya está visible de forma permanente en la barra lateral.
- (−) Ninguna de las cuatro correcciones se puede cubrir con las pruebas de Node actuales: las tres primeras necesitan el motor en marcha y la cuarta, saber qué se pinta. Se añaden a `docs/07` como comprobaciones manuales de cada entrega.

---

## ADR-025 — El borde borroso no era el modelo: era un ida y vuelta de reescalado (y el motor real, por fin, en marcha)

**Contexto.** El usuario pidió tres cosas: que el upscaling se pareciera a una imagen de referencia que había hecho con otra herramienta, que desapareciera el **borde borroso** que le fastidia de Upscayl, y una búsqueda de errores por todo el proyecto.

### 1. El borde borroso: causa medida, no sospechada

En lugar de leer el código y suponer, se midió la imagen de salida contra el original. El perfil de un borde era elocuente:

```
original (128 px)   0    0    0    2  254  255
salida (512 px)     0    0   11   11   11   11  242  242  242  242  255  255  255  255
```

Cada píxel de origen se repite **exactamente 4 veces** (vecino más cercano) pero con el valor cambiado: el negro `0` salía como `11` y el blanco `254` como `242`, siempre hacia el centro. No era un borde suave: era la imagen entera **lavada**.

La cuenta cuadró al decimal. La etapa `lineclean` de los pipelines de dibujo hace: modelo x4 → `su_imageio::resize` a tamaño original con Lanczos3 → `blend` al 70%. Con el backend de referencia —vecino más cercano—, subir a x4 y bajar con Lanczos3 **no reconstruye nada**: es un filtro paso bajo. El `blend` le da el 70% del peso, así que el resultado es una versión emborronada del original. Reproducido en un cuaderno con las mismas cuatro operaciones, la diferencia con la salida real fue de **0,22/255 de media**.

Dos agravantes: `su_imageio::resize` pasaba por `to_dynamic_image()`, que **cuantiza a 8 bits por canal** (el comentario de `photo:2x` promete 16 bits y el código hacía 8), y el campo `kernel` de las etapas `resize` existía en el JSON **y no se leía nunca**.

### 2. El motor real, que nunca se había ejecutado

La causa de fondo era otra: el binario que la aplicación encontraba estaba compilado **sin** la feature `onnx`, así que el único backend disponible era vecino más cercano. Se comprobó lo que faltaba, en este orden: la biblioteca de ONNX Runtime (el sistema tiene la 1.22.2, y `ort` 2.0-rc.13 pide la **1.28**), un modelo (18 MB, hash verificado contra el manifiesto embebido, que resultó ser **exacto**: `82f458db…`) y el planificador de tiles. Con los tres, la imagen de 128 px a x4 salió en **2,9 s en CPU**, con la calidad de la referencia externa:

| | ancho de transición | superficie plana | colores únicos |
|---|---|---|---|
| referencia del usuario | 2,63 px | 94,1% | 8 442 |
| **con el modelo de anime** | **2,20 px** | **93,6%** | **10 063** |
| con el motor clásico | 6,30 px | 87,8% | 13 017 |
| antes (lavado) | 1,00 px (todo el plano era un degradado) | 94,6% | 2 040 |

### 3. Los demás defectos encontrados

- **`fallbackModel` no se leía.** Los seis pipelines embebidos declaran un modelo de reserva, `docs/04` lo documenta y el runner no lo miraba: si el usuario instalaba el sustituto y no el principal, la imagen fallaba con «modelo no disponible». Ahora se usa y **se dice**.
- **Un modelo ausente tumbaba la imagen entera aunque la etapa fuese opcional.** En modo Fotos, con la imagen ligeramente ruidosa, el análisis activa la reducción de ruido; si `scunet-color` no está (no se puede descargar: su export reparte los pesos en dos archivos), el trabajo fallaba con `SU-E110` **antes** de llegar al escalado, con el modelo de escalado perfectamente disponible. Ahora una etapa que no aporta escala se omite con su motivo y la cadena continúa.
- **El proveedor no decía la verdad sobre sí mismo.** Sin la feature `onnx` se anunciaba a sí mismo como «referencia»; en la interfaz se mostraba el EP *recomendado* por el hardware en lugar del motor *usado*. Ahora el nombre incluye el filtro (`clasico-catmullrom`) y `GET /v1/capabilities` devuelve `engine`, que es lo que la barra lateral muestra.
- **El reescalado perdía el perfil ICC y la orientación.** Los dos se quedaban por el camino en cada etapa `resize`.
- **Los motivos de cada imagen no llegaban a la pantalla.** El sidecar calcula, desde el primer día, qué etapas se omitieron y por qué; el renderer recibía `executed` y `skipped` en cada `itemCompleted` y los **descartaba**. El usuario tenía delante una imagen distinta de la que había configurado, sin forma de saber por qué.

**Decisión.**

- **Ningún motor sin modelos puede llamarse a sí mismo «referencia».** `MockBackend` (vecino más cercano) se queda donde estaba su valor: las pruebas de geometría y composición. Para producción sin modelos se añade `ClassicalBackend`, un interpolador en `f32`, y `ClassicalProvider` con **Catmull-Rom** por defecto. La elección está medida, no elegida por gusto: sobre el dibujo de líneas, Lanczos3 y Catmull-Rom dan la misma transición (6,2 px frente a 6,3) pero Catmull-Rom conserva más superficie plana (87,8% frente a 83,1%), y en color plano cada píxel que deja de ser plano es un anillo alrededor de una línea.
- **Se dice lo que un interpolador no puede hacer.** Sobre un trazo de 1 px, cualquier interpolador reparte el escalón en una rampa de unos 4 px de origen. Recuperar un borde duro es deconvolución, y eso es lo que aprende un modelo. Se probó a forzarlo con máscara de enfoque de radio 2, 4 y 8 y cantidades 0,6-1,6: el mejor caso baja a 4,4 px y pierde 9 puntos de superficie plana. Queda escrito aquí y en el nombre del motor, en lugar de disimularlo.
- **El reescalado se hace en `f32`.** `to_dynamic_image_f32()` construye una imagen `Rgb32F`/`Rgba32F` y se reescala eso; el camino de 8 bits queda solo para **escribir**, que es donde el códec lo necesita. Se preservan ICC y orientación.
- **`kernel` se lee.** Un pipeline que pida `nearest` (arte de píxeles) o `catmullrom` recibe eso; `bicubic` se acepta como sinónimo de `catmullrom`, que es el nombre de la familia.
- **El runtime se comprueba antes de aceptar trabajo.** `probe_runtime` carga la biblioteca con `ort::init_from` y, si no puede, el sidecar lo avisa por `stderr` y por el registro, y sigue con el motor clásico. Esto no es cosmético: en `ort` con `load-dynamic`, la ruta perezosa termina en un `expect` que **aborta el proceso**; comprobar antes es la diferencia entre degradar y morir. La búsqueda es `ORT_DYLIB_PATH` → `<dataDir>/runtime/` → junto al ejecutable (la convención de `docs/06`), con soporte para el nombre versionado de Linux.
- **El proceso principal le pasa la ruta al sidecar.** `apps/desktop/src/main/sidecar/ort.ts` resuelve el archivo y el supervisor lo pone en `ORT_DYLIB_PATH`. Sin esto, `dlopen("libonnxruntime.so")` no mira el directorio del ejecutable y el sidecar caía al motor clásico sin decir nada.
- **`scripts/package.mjs` compila con `--features onnx`.** La feature es de compilación y la biblioteca es de ejecución: confundirlas producía instaladores que no podían inferir **nunca**. El script copia además la biblioteca al lado del binario si la encuentra, y avisa si no.
- **Los motivos de cada imagen se muestran.** `QueueItem.notes` recoge el `skipped` del motor, la lista muestra un contador en las imágenes con notas y el resumen final las lista. Es la respuesta a «¿por qué esta imagen salió distinta?», que hasta ahora solo existía en la API.

**Consecuencias**

- (+) Verificado con la ventana real de Electron y el modelo de anime instalado: el trabajo usa `realesrgan-x4plus-anime-6b`, termina en `Completed`, la salida mide 192×128 (48×32 a x4) y la página no produce **ni un error**. Las notas aparecen en pantalla.
- (+) Verificado que la ausencia de runtime no rompe nada: con `ORT_DYLIB_PATH` apuntando a un archivo inexistente, el registro dice `aviso: ORT_DYLIB_PATH apunta a '…', que no se pudo cargar: dlopen failed` y el trabajo se completa con interpolación clásica.
- (+) `fallbackModel` y la omisión de etapas sin escala tienen pruebas propias con un proveedor que falla solo para ciertos modelos. Sin eso habría que descargar 100 MB por caso.
- (+) 307 pruebas del workspace (326 con `--all-features`) y 68 de TypeScript.
- (−) **El resultado por defecto sigue sin ser el mejor posible.** Sin modelos instalados, el motor es un interpolador: mejor que antes (no lava los planos), pero con transiciones de 6 px. La calidad de la referencia exige descargar el modelo de anime en el gestor de modelos. Es una decisión de producto, no técnica: el proyecto no redistribuye pesos de terceros, y el aviso de licencia (CC-BY-NC-SA en dos modelos) se muestra antes de descargar.
- (−) El runtime no se descarga solo. Quedó instalado junto al binario del sidecar para verificarlo, y `docs/06` explica dónde ponerlo. Automatizar la descarga del runtime (como se hace con los modelos) es trabajo pendiente.
- (−) GPU sin usar. Este equipo tiene una RTX 4060, pero la biblioteca de ORT disponible es la de CPU y falta cuDNN para CUDA 13: se prefirió **verificar calidad** antes que perseguir rendimiento.

---

## ADR-026 — El modelo se descarga en el primer uso, y su ausencia ya no cuesta la imagen

**Contexto.** El usuario informó de que «ahora falla otra vez» al escalar. El registro de la aplicación mostraba el motor con ONNX Runtime **cargado**, la ventana en pie y el trabajo aceptado; lo que faltaba estaba en otro sitio: `~/.local/share/superupscaly/models/` **no existía**. Reproducido desde el CLI con un directorio de datos vacío:

```
$ su-cli upscale imagen.png --mode illustration --scale 4 --data-dir /tmp/vacio
    fallo SU-E110: model not available: /tmp/vacio/models/realesrgan-x4plus-anime-6b-82f458db.onnx
Terminado: Failed (0 correctas, 1 fallidas)
```

Es el peor orden posible de descubrimiento: la aplicación arranca, deja arrastrar una imagen, el botón se habilita, se pulsa, y el resultado es un fallo por algo que el usuario no sabía que tenía que instalar. Y no es un caso raro: **es el estado de cualquier instalación nueva**, porque el runtime viaja con la aplicación y los pesos no.

La consecuencia de no tener el modelo también se midió, sobre la imagen del usuario:

| motor | ancho de transición del borde |
|---|---|
| con `realesrgan-x4plus-anime-6b` | **2 px** (255 · 60 · 0) |
| referencia externa del usuario | 2 px (255 · 35 · 0) |
| interpolación clásica | 6 px (255 · 247 · 216 · 160 · 95 · 39 · 8 · 0) |

O sea: el «borde borroso» que el usuario quería quitar y la falta del modelo son **el mismo problema**. Ninguna mejora del interpolador lo resuelve (se probaron máscaras de enfoque de radio 2, 4 y 8: el mejor caso baja a 4,4 px y pierde 9 puntos de superficie plana, ver ADR-025).

**Decisión.**

- **La aplicación descarga el modelo que falta antes de aceptar el trabajo.** `apps/desktop/src/main/models/ensure.ts` pregunta a `/v1/capabilities`, `/v1/models` y `/v1/pipelines` y descarga **solo los modelos de las etapas que escalan**. Hacerlo en el proceso principal y no en el renderer es deliberado: la garantía vale para cualquier cliente, no solo para la ventana que la pide.
- **Que modelo hace falta lo dice el motor.** Los identificadores salen de `/v1/pipelines`. Escribirlos en TypeScript habría sido la tercera copia de la misma regla (ver ADR-028, que retira una). El modo Manual se respeta: si el usuario fijó un modelo, se descarga **ese**, porque es el que el runner va a usar.
- **Solo lo imprescindible.** Las etapas de restauración (limpieza de líneas, rostro) ya se omiten solas con su motivo cuando falta su modelo, así que no se descargan por adelantado: entre 18 y 34 MB que no cambian el resultado.
- **Un fallo de descarga no cancela el lote.** Se anota en el registro y el trabajo sigue. El motor tiene respaldo (abajo) y el informe dice con qué se hizo; perder las imágenes porque el wifi va mal sería peor que dar un resultado interpolado — y decirlo.
- **Un motor que no puede cargar un modelo interpola en lugar de fallar.** `JobContext` gana `fallback`, y `Providers::new` centraliza cuál es: el mismo `ClassicalProvider` del ADR-025. El reintento es **uno solo** y solo para `ModelMissing`, `ModelHashMismatch`, `ExecutionProviderUnavailable` y `TensorRtEngineBuildFailed`. Los fallos de memoria **no** entran: esos los resuelve el runner bajando el tile, y mandarlos al respaldo cambiaría calidad por un problema pasajero. Si el respaldo tampoco puede, se devuelve **el error original**, que es el que dice qué modelo falta.
- **El item queda marcado como degradado y la nota nombra el respaldo.** «¿Por qué esta imagen salió distinta?» tiene respuesta en la lista y en el resumen, igual que el resto de motivos.
- **La interfaz dice que está descargando.** Entre el clic y el primer tile hay segundos (18 MB): la barra muestra «Preparando: descargando el modelo» con el porcentaje real. Se lee del estado de descargas que ya existía, sin inventar un canal nuevo.

**Consecuencias**

- (+) Verificado con la ventana real y el directorio de modelos **borrado**: al pulsar «Upscaly» la aplicación descargó `realesrgan-x4plus-anime-6b` (18 406 820 bytes, hash del manifiesto comprobado), el trabajo terminó en `Completed` y la salida de 512×512 tiene el borde de la referencia (255 · 60 · 0). Sin un solo error en la página.
- (+) Verificado que el aviso aparece y avanza, con la barra leída de la ventana durante la descarga: `Preparing: downloading the model · realesrgan-x4plus-anime-6b · 0%` → `9%`. Con el modelo ya instalado no aparece y el trabajo empieza de inmediato.
- (+) Verificado desde el CLI con el directorio vacío: el trabajo pasa de `Failed` a `Completed (degradado)`, con la nota `motor de respaldo 'clasico-catmullrom': el elegido no pudo usarse (model not available: …)`.
- (+) Dos pruebas nuevas del respaldo en `su-jobs` (uno de ellos falla si el contexto no tiene respaldo, para que el comportamiento anterior siga siendo el de antes cuando nadie lo pide) y doce del módulo de garantía.
- (−) La primera imagen de un modo tiene una espera de red. Es el precio de ser honesto con los pesos de terceros; el aviso de licencia de cada modelo sigue en el gestor.
- (−) El runtime de ONNX Runtime sigue sin descargarse solo (ADR-025).

---

## ADR-027 — Un píxel transparente no tiene color

**Contexto.** La imagen del usuario es un PNG con transparencia, como buena parte del arte que se escala. El canal alfa se guardaba aparte y se recomponía al final (bien), pero el **RGB de los píxeles transparentes entraba tal cual en el modelo**. Medido en su archivo: los píxeles con alfa 0 son `(0,0,0)` —negro— y el dibujo es un marco claro con un contorno blanco. El modelo, que no ve el alfa, recibe un fondo negro y reconstruye un contorno entre ese negro y el dibujo: un color que **nadie eligió**.

**Decisión.** Antes de inferir, `su_imageio::bleed_transparent` **extiende el color de los píxeles visibles hacia la zona transparente** (difusión por niveles, ocho vecinos: el color que gana es siempre el del píxel visible más cercano). Es lo mismo que hace un relleno por difusión.

Lo que **no** hace, y es la parte que importa: no toca los píxeles con alfa distinto de cero ni el canal alfa. Un píxel semitransparente sí aporta color a la mezcla, y cambiarlo cambiaría la imagen compuesta que el usuario ve; esto solo rellena lo que no se ve. Sin canal alfa, el resultado es el mismo objeto, sin copia.

**Consecuencias**

- (+) Cinco pruebas nuevas, incluida la que fija la propiedad que hace segura la operación: un píxel semitransparente conserva su color, y el alfa no se modifica.
- (+) El alfa que se escribe sigue siendo el del original reescalado, así que la silueta no cambia. **(Superado en parte por ADR-029: el alfa ya no se reescala aparte con Lanczos, lo reconstruye el modelo. Lo que este ADR fija sigue en pie: el relleno no toca ni el alfa ni los píxeles semitransparentes.)**
- (−) Sobre la imagen del usuario el efecto es pequeño (su zona transparente ya era negra, como el marco), y se dice así en lugar de venderlo como una mejora visible. Importa en los PNG cuyo fondo recortado es de otro color, que es donde el halo es evidente.

---

## ADR-028 — La cadena que dibuja el panel es la del motor

**Contexto.** El panel de ajustes avanzados dibujaba la cadena de etapas con `planChain`, una **copia en cliente** de las reglas del motor: elegía el modelo por modo con dos identificadores escritos a mano. Esa copia se quedó atrás en cuanto hubo más de una escala —a 2x el motor usa `2x-animesharpv3` y el panel anunciaba el de 4x— y dibujaba etapas que el análisis puede omitir. Es el defecto que el propio proyecto persigue: dos listas de lo mismo acaban diciendo cosas distintas.

Además, al verificarlo apareció un fallo que no tenía nada que ver con el panel: **si la ventana se abría con el motor ya en pie, el catálogo de modelos no se preguntaba nunca**. El refresco colgaba del aviso de cambio de estado, y ese aviso no llega si el motor arrancó antes de que la ventana se suscribiera. La lista de modelos y la cadena se quedaban vacías hasta que el usuario abría el gestor de modelos, que era la única otra cosa que las pedía.

**Decisión.**

- **La cadena se proyecta desde `/v1/pipelines`.** `lib/pipelinePlan.ts` pasa de decidir a **traducir**: recorre las etapas del pipeline real, las rotula con el vocabulario de la interfaz (`lineclean` → «Reduciendo ruido», `halve`/`upscale2` → «Escalando») y añade `decode` y `encode`, que no están en el pipeline porque no son decisiones. Se elimina el reparto de pesos, que era de la simulación retirada en el ADR-024.
- **Lo condicional se marca, no se promete.** Las etapas con `when` se dibujan con un «·?»: quien decide es el análisis de la imagen, con sus números. La verdad por imagen sigue estando donde estaba: `effectivePipeline` y las notas de cada item.
- **El catálogo se pregunta también al leer el estado inicial.** Si la primera lectura dice «listo», se refresca ahí mismo.

**Consecuencias**

- (+) Verificado en la ventana real, combinación por combinación, contra `GET /v1/pipelines`: a 2x el panel dice `2x-animesharpv3`, a 8x dibuja las dos pasadas, y en Fotos aparece `gfpgan-v1.4` en su sitio.
- (+) La interfaz ya no puede anunciar un modelo que el motor no va a cargar: no tiene de dónde sacarlo.
- (−) La proyección vive en el renderer y **no tiene pruebas automáticas**: `npm test` cubre el proceso principal y los scripts. Queda en el plan de pruebas como comprobación manual (§6).

---

## ADR-029 — La silueta la reconstruye el modelo, no un interpolador

**Contexto.** El interior de la imagen del usuario está «en muy buena calidad»; lo que falla es el contorno: «el borde se ve demasiado pixelado/borroso». No es una impresión. El color lo reconstruía el modelo con un escalón de 1 px mientras **el canal alfa se reescalaba aparte con Lanczos3**, que era el último tramo del ida y vuelta del ADR-025. Medido en el borde superior de su dibujo:

```
origen (alfa)               0 · 0 · 0 · 70 · 255
antes (Lanczos3)            0 · 1 · 2 · 1 · 0 · 0 · 0 · 0 · 0 · 5 · 19 · 45 · 85 · 138 · 194 · 239 · 255
Upscayl (su referencia)     0 · … · 1 · 0 · 0 · 116 · 255
ahora (el modelo)           0 · … · 0 · 0 · 115 · 255
```

La rampa pasa de 4–7 px —con anillo, los `1 · 2 · 1` que preceden al escalón— a 0–1 px. Una silueta con una rampa de siete píxeles se ve como un halo alrededor de todo el contorno, por nítido que esté el interior; y en la banda del borde, compuesta sobre fondo oscuro, la diferencia media con la referencia era de **5,42/255** y ahora es de **0,98/255**. La posición del borde no se movió: coincide con la referencia en todas las columnas medidas.

**Opciones evaluadas**

| Opción | Resultado |
|---|---|
| Dejarlo como estaba (Lanczos3) | Rampa de 4–7 px. Es el defecto que se está arreglando |
| Vecino más cercano | Rampa de 1 px, pero escalones de 4 px en cada diagonal: cambia «borroso» por «pixelado», que es la otra mitad de la queja |
| Endurecer el alfa con un remapeo de contraste alrededor de 0,5 | Barato y sin anillo, pero destruye el alfa **que sí es suave**: plumas, sombras y degradados desaparecen |
| Máscara de enfoque sobre el alfa | Deja anillo por sobreoscilación y valores fuera de rango que hay que recortar |
| **El mismo modelo sobre el alfa, como imagen de tres canales** | Rampa de 0–1 px, sin anillo, y conserva los degradados. Es lo que hace `chaiNNer` |

**Decisión.** La silueta recorre **la misma cadena de escalado que el color**, dentro de `run_pipeline`: mismo modelo, mismo tile, mismo solape, misma malla. No es un reescalado posterior: es la segunda mitad de cada etapa que cambia la geometría.

Tres reglas que salieron de medir, no del gusto:

- **Solo la llevan las etapas que cambian la geometría.** Una etapa de restauración usa un modelo x4 y devuelve la imagen a su tamaño; hacerla pasar por ahí cuesta una inferencia de más y, sobre todo, un reescalado de vuelta que vuelve a difuminar el contorno. Medido en el pipeline de dibujo: `lineclean` metía el alfa en un modelo x4 y lo reducía otra vez, así que el trabajo tardaba 5,1 s y el contorno salía blando igual. Con la regla, 3,5 s y el contorno duro. Lo decide `stage_is_size_neutral`, la misma función que decide si una etapa puede omitirse.
- **El alfa no se mezcla.** `blend` es una operación de color —mezclar la salida de un restaurador suaviza el color—: la silueta no es un color, es la forma de la imagen.
- **La silueta solo se escribe si tiene el tamaño de la imagen.** Un alfa de otra medida no es una imagen algo peor, es una imagen corrupta: el motor devuelve un error interno con las dos medidas en lugar de dejar que el codificador la rechace más tarde y peor.

**Consecuencias**

- (+) El contorno de un PNG con transparencia queda como el de la referencia: rampa de 0–1 px y el borde en el mismo sitio.
- (+) Una imagen **sin** canal alfa no paga nada: la pasada extra existe solo cuando hay una silueta que reconstruir.
- (+) Los degradados de alfa se conservan. Medido con un alfa radial suave: el salto máximo entre píxeles vecinos es de 2/255 (un alfa endurecido daría más de 100) y el error frente al degradado ideal no pasa de 10/255; el interior sale exactamente plano (σ = 0,000).
- (+) Dos pruebas nuevas con un proveedor que cuenta **ejecuciones**, no peticiones: la etapa que escala corre el modelo dos veces —el color y la silueta— y la de restauración una.
- (+) Sin modelo, la silueta la interpola el mismo motor que el color. Un resultado degradado lo está entonces de forma coherente —contorno e interior igual de suaves, y anunciado como degradado— en lugar de mezclar un contorno duro con un interior blando.
- (−) Una imagen con transparencia cuesta una pasada de inferencia más por etapa de escalado: medido, 3,5 s frente a 2,8 s en este equipo (128→512 en CPU). En un 8x son dos. Queda dicho en la guía de usuario, y no hay ajuste para desactivarlo porque el resultado sería volver al halo blando.
- (−) El alfa pasa por el modelo, así que también hereda sus rarezas: sobre un alfa sintético muy suave el modelo se desvía hasta 10/255 del ideal en los extremos de la rampa. Es un sesgo de ±4 %, no un escalón.

---

## ADR-030 — Las pistas de tiling del manifiesto llegan al runner, y el tile de CPU deja de ser el más pequeño

**Contexto.** Una revisión del camino de configuración encontró dos defectos que se multiplicaban entre sí.

1. **La sección `tiling` de cada modelo era código muerto.** `ModelEntry::tiling` se parseaba, se validaba y se probaba, y no llegaba nunca al runner: `RunnerConfig` se construía con `DEFAULT_CANDIDATES`, `overlap_divisor: 16`, `pad_to: 32` y ningún `vramPerMegapixel`. Un modelo que declara `candidates: [768, 512, 384]` porque el tile 1024 le hace producir artefactos seguía recibiendo 1024, y uno que declara `vramPerMegapixel: 250` se presupuestaba con el valor por defecto.

2. **Sin presupuesto de VRAM, el tile era el mínimo.** `initial_tile` buscaba "el menor candidato que cubra la imagen" y, cuando ninguno la cubría —justo el caso de una foto grande en CPU—, caía en un segundo `.min()` sobre los candidatos razonables. Para una imagen de 3000×2000 eso son **256 px** en lugar de 1024: del orden de 15×10 tiles con su solape y su composición, frente a 4×3. El comentario del propio código decía lo contrario de lo que hacía.

**Decisión.**

- Las pistas viajan **preguntadas al proveedor** (`BackendProvider::tiling_hints`), no copiadas en `RunnerConfig` por los tres sitios que construyen el runner. El proveedor es el único que sabe qué modelo se va a usar de verdad —el del pipeline, el de reserva o el que el usuario fijó a mano—, que es el mismo argumento de ADR-020 para `native_scale`. `RunnerConfig::with_hints` aplica las pistas a una **copia** por etapa, porque cada etapa usa un modelo distinto.
- Precedencia declarada, de más a menos: **manifiesto → calibración del equipo → estimación por defecto**. El manifiesto es lo que dice el autor del modelo; la calibración mide lo que consume en esta máquina; y el valor por defecto es el último recurso. Un manifiesto sin candidatos utilizables (todos por debajo de `MIN_TILE`) se ignora en lugar de dejar al planificador sin opciones.
- `initial_tile` sin presupuesto pasa a ser "el menor candidato que cubre la imagen y, si ninguno la cubre, **el mayor de los razonables**". La función se extrae a `choose_tile_without_budget`, que es pura y se prueba con los cuatro casos: imagen que cabe en todos, que cabe en algunos, que no cabe en ninguno y lista vacía.

**Consecuencias**

- (+) Las pistas de un modelo con `candidates` restringidos se respetan: se acaba la degradación silenciosa por un tile que ese modelo no admite.
- (+) Una imagen grande en CPU usa el tile grande. La prueba que fijaba el comportamiento anterior (`a_small_image_gets_a_small_tile`) se reescribió: **afirmaba lo que el código hacía, no lo que debía hacer**.
- (+) Hay tres pruebas nuevas de que las pistas llegan: candidatos al plan, `vramPerMegapixel` al presupuesto y `overlap_divisor`/`padTo` al plan.
- (−) El catálogo embebido **no declara todavía ninguna sección `tiling`**: el mecanismo funciona y está probado, pero hoy ningún modelo del catálogo lo aprovecha. Es una decisión de datos pendiente, no de código: los valores hay que medirlos por modelo, y el valor por defecto sigue siendo razonable.
- (−) El tile grande consume más memoria por tile. En CPU no hay VRAM que agotar y el presupuesto ya acota el caso de GPU, así que el riesgo es un pico de RAM mayor en imágenes muy grandes.

---

## ADR-031 — El peso de una etapa puede depender de lo que mide el análisis, y el umbral del enfoque estaba en otra escala

**Contexto.** Dos ajustes del pipeline no llegaban a producir el efecto que declaraban.

1. **El umbral del enfoque.** Las etapas `unsharp` declaran `threshold: 0.02` (foto) y `0.03` (dibujo), en la misma escala `0..1` que `amount`, `radius` y `blend`. El código lo dividía por 255 antes de compararlo con las diferencias entre píxeles. Con eso, el valor real era `0,0000784`: **el umbral no filtraba nada**. El enfoque amplificaba el ruido de las zonas planas —exactamente lo que el umbral existe para evitar— y la diferencia entre el 2 % de la foto y el 3 % del dibujo no producía ningún efecto observable.
2. **El peso del denoise era fijo.** La etapa declaraba `blend: 0.9` para cualquier ruido: la misma reducción para una foto con ruido moderado —donde un 0.5 conserva el detalle— que para una degradada, donde conviene el 1.0. El análisis ya medía el ruido que hacía falta para decidir; ese dato no se usaba.

**Decisión.**

- El umbral pasa a leerse en la escala del resto del pipeline, sin dividir, y se documenta la escala en `docs/04`. Un valor negativo se recorta a cero.
- La etapa gana `blendFrom: { path, min, max }`: el peso se interpola entre `min` (variable a 0) y `max` (variable a 1) con la variable recortada a `0..1`. **Una ruta que no existe, o que no es numérica, es un error**, no un cero silencioso: una errata en `analysis.noisse` dejaría la etapa sin efecto sin que nada lo dijera.
- Los tres pipelines de foto declaran en su etapa de reducción de ruido `blendFrom: { path: "analysis.noise", min: 0.5, max: 1.0 }`.
- El peso se resuelve en **un solo sitio** (`Stage::blend_weight`), incluido el de la restauración facial, para que dos caminos no acaben discrepando.

**Consecuencias**

- (+) El enfoque deja de amplificar el ruido de las zonas planas, y los dos umbrales del catálogo vuelven a significar algo distinto.
- (+) Una prueba mide la propiedad, no la ausencia de error: el mismo pipeline sobre la misma imagen da **resultados distintos** con ruido 0.4 y con ruido 0.95. Con el peso fijo eran idénticos. Otra prueba recorre los tres pipelines de foto y exige que el peso crezca con el ruido.
- (+) `BlendFrom::weight` se prueba con los dos extremos, el punto medio, un valor fuera de rango (se recorta) y tres rutas inválidas: dos inexistentes y una booleana.
- (−) **La etapa de denoise no se ejecuta hoy en producción**: su modelo (`scunet-color`) no tiene URL en el catálogo porque los únicos exports a ONNX reparten los pesos en dos archivos (ADR-019). El peso variable sí actúa cuando el equipo está en modo degradado. El mecanismo queda listo para cuando el modelo se instale a mano con `localPath`.
- (−) Cambiar el umbral del enfoque altera el resultado de todas las imágenes. Es el objetivo de la corrección, pero conviene saberlo al comparar con una versión anterior.

---

## ADR-032 — `onlyOnFaces` significa lo que dice: se restauran caras, no imágenes

**Contexto.** Los tres pipelines de foto declaran su etapa facial con `onlyOnFaces: true` y `blend: 0.85`. La documentación prometía desde el principio que la restauración facial "se compone solo sobre las cajas detectadas, con mezcla suave en los bordes de la máscara". El runner leía el campo y **lo ignoraba**: pasaba la imagen completa por GFPGAN.

El efecto es el peor de los dos mundos. En una foto de 20 MP, cada cara ocupa una fracción mínima de la entrada, así que el modelo ve un par de píxeles por rostro y lo único que puede hacer es repartir color; y se paga una pasada completa del modelo más pesado del catálogo (340 MB). De las dos cosas que la documentación prometía —recorte por cara y mezcla enmascarada— no había ninguna.

**Decisión.** Se implementa la ruta real, con las tres piezas que la hacen verificable: **recorte → inferencia por cara → pegado con máscara**.

- **Detección.** `EvalVars` gana `faces: Vec<FaceBox>`, y `build_vars` la rellena filtrando por el mismo criterio (`is_reliable`) que cuenta `face_count`, para que el número y las cajas no puedan discrepar. Las cajas que se solapan más de la mitad del área menor se **fusionan**, iterando hasta que el resultado deja de cambiar: un detector puede devolver dos cajas por rostro, y dos caras muy juntas producen una tercera fusión al unirlas.
- **Recorte.** Cuadrado, con un 35 % de margen alrededor de la caja y llevado a **512×512**, que es el tamaño con el que GFPGAN se entrena y se exporta. El recorte se **desplaza** para caber en la imagen en lugar de recortarse: una cara pegada al borde tiene que entrar entera. Por debajo de 96 px de recorte la cara se deja como estaba y se cuenta aparte, porque a ese tamaño el modelo inventa en lugar de restaurar.
- **Pegado.** Máscara radial que vale 1 en todo el rostro y cae a 0 en el borde del recorte, con transición suavizada (`smoothstep`). El radio se normaliza por el **semilado** y no por la diagonal: normalizando por la diagonal, el radio máximo fuera de las esquinas se queda en 0,98 y la máscara no llega a anularse en los puntos medios de los bordes, que es donde el pegado dejaría una costura recta.
- **Peso.** La intensidad que eligió el usuario (`prefs.faceRestore`: 0.6 suave, 0.85 automático, 1.0 alta) multiplicada por el `blend` de la etapa, que pasa a **1.0** en los tres pipelines: la preferencia es la que decide, y el `blend` queda como techo de autoría. El `blend` fijo de 0.85 recortaba la preferencia "alta" a 0.85 sin decirlo.
- **Omitida con motivo.** Sin cajas, la etapa se reporta como omitida con "no se detectaron caras"; con caras demasiado pequeñas, con cuántas quedaron fuera.

**Consecuencias**

- (+) La etapa hace lo que anuncia, y hay pruebas de cada parte: fusión de cajas, margen y ajuste al borde del recorte, y una prueba de extremo a extremo con un backend que devuelve un color plano —el centro del rostro cambia, la esquina de la imagen no, y el margen queda en un valor **intermedio**, que es lo que separa un pegado con máscara de un parche rectangular.
- (+) El coste pasa a ser proporcional al número de caras: una foto sin caras no paga la pasada del modelo (ya la pagaba antes, sin ganar nada).
- (−) **Sin detector instalado no hay caras que restaurar, y hoy no hay detector.** `su-analyze` devuelve `faces` vacío desde la Fase 2 —la detección "llega en la Fase 4" según su propio comentario—, así que en producción la etapa se omite con su motivo y el usuario lee "no se detectaron caras". El trabajo que queda no es del runner sino del análisis: implementar el detector (`yunet-2023`, ya previsto en `docs/04`, 337 KB, Apache-2.0) y añadirlo al catálogo embebido. Se prefiere decir esto a fingir que la funcionalidad está completa.
- (−) La máscara radial asume que la cara llena una parte razonable del recorte. Con un detector que devuelva una caja desproporcionada (mucho más alta que ancha), la transición se estrecha en un lado. El radio interior se recorta a 0,95 para que la máscara no se quede sin transición.
- (−) La restauración facial no toca el canal alfa: la máscara describe cuánto color se sustituye, no la forma de la imagen.

---

## ADR-033 — La copia que nadie leía, y la luma que no era luma

**Contexto.** Dos defectos de memoria y uno de precisión, ninguno visible en el resultado pero los tres costosos.

1. **`bleed_transparent` copiaba la imagen entera.** Para una imagen **sin** canal alfa —la mayoría de las fotos—, la primera línea era `return image.clone()`. En una imagen de 20 MP, ese clon son unos 240 MB de `f32`, y se hacía siempre, porque `process_item` la llama sin preguntar.
2. **`run_pipeline` clonaba la entrada para no usarla.** El bucle empezaba con `let mut current = source.clone()`, y después la primera etapa sustituía `current` por una imagen nueva: el clon se tiraba sin llegar a leerse. Sumado al anterior, una foto podía tener tres o cuatro copias vivas a la vez.
3. **"Luma" era la media de los tres canales.** `validate_output` detecta el caso "el modelo devolvió negro o un color plano" con una desviación estándar por debajo de un mínimo. Medía los tres canales mezclados y lo llamaba luma. Un rojo plano uniforme —una imagen tan plana como una negra— daba 121/255 y **pasaba el filtro**.

**Decisión.**

- `bleed_transparent` devuelve `Cow<DecodedImage>`: prestado cuando no hay alfa ni nada que rellenar, propio solo cuando hay que construir una imagen nueva.
- La imagen que circula por el pipeline es `Option<DecodedImage>`: `None` significa "todavía es la de entrada". La primera etapa que construye una imagen la sustituye, y la entrada no se copia nunca. El `unwrap_or_else(|| source.clone())` del final se conserva para el caso —imposible en los pipelines reales— de una cadena sin ninguna etapa que sustituya la imagen.
- La uniformidad se mide sobre la **luma real** (Rec. 709: 0,2126 R + 0,7152 G + 0,0722 B), en una función propia (`luma_std_dev`) y no dentro de la validación, para que la prueba pueda comparar el número medido con el que debería salir. El mínimo, el máximo y el rango dinámico siguen midiéndose sobre los canales, porque un rango colapsado es un problema de cuantización del buffer, no de brillo.

**Consecuencias**

- (+) Una foto sin transparencia no paga ninguna copia antes de la inferencia, y el pico de memoria en un lote de imágenes grandes baja de forma proporcional.
- (+) Un color plano se detecta con independencia de su tono: hay una prueba con rojo, verde y azul planos que ahora se rechazan.
- (+) La prueba de la métrica es una **propiedad numérica**: dos mitades que difieren 0,4 en el canal rojo tienen que dar una desviación de luma de `0,2126 × 0,4 × 255 / 2 = 10,84`, y se comprueba con las constantes del código en lugar de repetir los coeficientes.
- (+) Se comprueba el otro lado: un degradado que solo mueve el canal azul sigue siendo válido. Medir luma no puede convertirse en rechazar imágenes buenas.
- (−) `bleed_transparent` cambia de firma: `Cow` obliga a decidir en el llamador si hace falta la propiedad. Es una línea más y evita que la copia vuelva por descuido.
- (−) `run_pipeline` con `Option<DecodedImage>` es un poco más indirecto de leer. Se paga a cambio de no copiar 240 MB por imagen, y está explicado donde se declara.

---

## ADR-034 — Dos carreras y un aviso de arranque inventado

**Contexto.** Los tres defectos comparten la misma forma: una comprobación y una acción que no son atómicas, o un error que se interpreta como "no puedo" cuando significa "ya estaba".

1. **Pausar o cancelar podía responder un error interno.** El manejador preguntaba `is_alive(id)` y luego llamaba a `pause(id)`, que volvía a buscar el control del trabajo. Si el hilo terminaba entre las dos llamadas, la segunda no encontraba nada y devolvía `Internal`, que la API traducía a **500**. Es la misma clase de fallo que ADR-024 vino a eliminar —"pausar algo que acaba de terminar no es un fallo del motor"— y quedaba una ventana de milisegundos por la que podía volver.
2. **El registro se instalaba una sola vez.** `su_telemetry::init` llamaba a `try_init`, que falla si ya hay un subscriber global, y ese error se imprimía como "aviso: sin registro en archivo" y el programa seguía. Es un problema de arranque disfrazado de aviso benigno.

**Decisión.**

- El gestor de trabajos decide **en una sola operación**. `pause`, `resume` y `cancel` devuelven `ControlOutcome::{Applied, NotRunning}` en lugar de lanzar cuando no hay hilo: `NotRunning` significa "la orden se quedó sin efecto porque el trabajo ya no está en curso", que no es un error. Un identificador que **no existe** sigue siendo un error, con un mensaje que ya no se puede confundir con el otro caso ("no existe el trabajo 'x'"), y el manejador lo traduce a 404. `resume_or_restart` pasa a decidir con esa misma respuesta en lugar de con `is_alive`: si el trabajo terminó justo antes, reanudar lo relanza (que es lo que el usuario pidió) en lugar de fallar.
- Ninguna orden produce efectos cuando no hay hilo: no se cambia el estado ni se emite un evento. Antes, pausar algo ya terminado escribía `Paused` en un trabajo completado.
- `su_telemetry::init` pasa a ser **idempotente**: si ya hay un registro global, devuelve la ruta del log del día sin error, y solo informa de fallo cuando de verdad no pudo instalarlo. La ruta sigue siendo la del archivo que use quien lo instaló.

**Consecuencias**

- (+) La ventana desaparece por construcción: no hay dos llamadas que puedan discrepar.
- (+) Hay una prueba de la carrera, buscada a propósito: 25 trabajos de un solo ítem con una pausa y una cancelación inmediatas, exigiendo `200` en todos. El resultado de cada intento depende de quién llegue antes —pausar en marcha también es válido—, así que la prueba exige lo único que no puede pasar: un `500`.
- (+) Otra prueba unitaria fija el estado esperado: sobre un trabajo terminado, `pause`, `cancel` y `resume` devuelven `NotRunning` y **no** cambian el estado del trabajo ni el de sus ítems. Y `resume_or_restart` sigue relanzando, que es lo que separa "reanudar" de "despausar".
- (+) La prueba de telemetría instala el registro dos veces y comprueba que las líneas de las dos etapas acaban en el archivo, con el directorio personal enmascarado. Se hace en una sola prueba porque el registro es global al **proceso**.
- (−) `ControlOutcome` es API pública nueva del gestor. Se acepta: es lo que permite que el llamador no tenga que adivinar, y el `enum` es de dos variantes.
- (−) Un trabajo terminado sigue sin poder "despausarse" con `resume`. Es deliberado: relanzarlo es `resume_or_restart`, y mezclar las dos cosas fue lo que produjo la carrera.

---

## ADR-035 — Una imagen del disco llega a la ventana por lista de autorización, no por `file://`

**Contexto.** La interfaz tiene que mostrar dos imágenes que están en el disco: la original, para compararla, y el resultado que acaba de escribir el motor. La ventana no carga desde `file://` sino desde `app://superupscaly`, y su CSP declara `img-src 'self' data: blob:` (ADR-004 y `csp.ts`). Es decir: el renderer **no puede** leer una imagen por su ruta, y no es un descuido — es lo que hace que `'self'` signifique algo.

Las salidas posibles eran tres, y las tres se pueden defender mal:

1. **Añadir `file://` a la CSP.** Es lo más corto y lo peor: convierte el origen propio en decorativo y deja cualquier inyección en la página a un `src` del disco entero.
2. **Pasar los bytes por IPC y dibujarlos como `blob:`.** No toca la CSP, pero copia la imagen entera —20 MB de PNG, 250 MB ya decodificada— por el canal de mensajes cada vez que se abre la comparación, y deja la copia viva en el renderer.
3. **Servirla por el esquema que ya es `'self'`.**

**Decisión.** Se sirve por el propio esquema, en `app://superupscaly/media?p=<ruta>`:

- **Solo se sirve lo que el proceso principal autorizó antes.** El renderer no puede añadir entradas: pide rutas que ya conoce. Se autorizan en los dos únicos momentos en que una imagen pasa a interesar: cuando termina de validarse (es una imagen de la cola) y cuando el motor informa de que escribió un resultado. Una ruta inventada no está en la lista, y la respuesta es `403` con su línea en el registro.
- **La comprobación de escapada del directorio del renderer no aplica a esta ruta**, y no hace falta: no se compone ninguna ruta con lo que llega, se busca una coincidencia exacta. Lo que viaja en la URL no se usa para abrir nada que no estuviera ya autorizado.
- **`Cache-Control: no-store`.** Un escalado repetido sobre el mismo archivo de salida usa exactamente la misma URL. Con la caché puesta, la comparación seguiría enseñando el resultado anterior: una imagen que se ve, sin error, y que no es la que se acaba de escribir.
- **El formato de la URL vive en `packages/shared/src/media.ts`**, porque lo construye la interfaz y lo interpreta el proceso principal. Con dos versiones del formato, una discrepancia —un parámetro renombrado, una barra de más— dejaría la comparación en blanco sin decir por qué.

**Consecuencias**

- (+) Ninguna línea de configuración nueva de seguridad: la CSP no se toca, y la superficie que se abre es una lista de rutas concretas en lugar de un lector de archivos.
- (+) La lista crece con el uso y se comprueba por cadena exacta, así que no hay canonicalización que pueda convertir una ruta válida en otra distinta de la que viaja en la URL.
- (+) Hay pruebas del registro y del contrato de URL: ruta no autorizada, coincidencia exacta, forma de ruta inválida, ida y vuelta con espacios, acentos, almohadilla e interrogación, y URL de otro origen o de otro esquema.
- (−) La lista vive en memoria mientras la aplicación está abierta. No se vacía y no se persiste: una imagen que se quitó de la cola sigue pudiéndose servir hasta que se cierre la ventana. Se acepta porque solo alcanza a archivos que el usuario ya eligió en esa sesión, y el coste de persistirla sería tener que invalidarla.
- (−) Un formato que la ventana no dibuja (TIFF, BMP) no se puede previsualizar, y una imagen enorme puede no caber en memoria. Los dos casos se detectan al cargar y se **dicen**, cada uno con su motivo, en lugar de dejar un hueco.

---

## ADR-036 — Las importaciones del paquete compartido llevan la extensión, para que las pruebas puedan cargarlo

**Contexto.** `packages/shared` es la fuente única de los contratos: tipos, guardas de la frontera IPC, el catálogo de errores, los diccionarios y, desde ADR-035, el formato de la URL de medios. Hasta ahora ninguna prueba del proceso principal lo cargaba **en ejecución**: los `import type` desaparecen al borrar los tipos, así que la resolución nunca llegaba a intentarse.

Al escribir la prueba del registro de medios apareció la consecuencia: `npm test` ejecuta Node con `--test` sobre el TypeScript directamente, y Node no completa las extensiones que falta (`export * from './types'` no resuelve). El paquete compila y se empaqueta —esbuild y Next sí las completan—, pero no se puede cargar desde una prueba.

**Decisión.** Todas las importaciones relativas **dentro** de `packages/shared` nombran el archivo con su extensión, y `tsconfig.base.json` habilita `allowImportingTsExtensions` (que TypeScript solo admite sin emitir ficheros, y aquí nadie emite: el proceso principal lo empaqueta esbuild y el renderer, Next).

**Consecuencias**

- (+) El código compartido se puede **probar de verdad**, no una copia: el registro de medios, la guarda de rutas y el contrato de URL se ejecutan en las pruebas tal como se ejecutan en la aplicación.
- (+) La convención no es nueva: `apps/desktop/src/main/**/*.test.ts` ya importaba `./ensure.ts` con extensión por el mismo motivo. Ahora es una regla del proyecto en lugar de un detalle de un archivo.
- (−) Cada importación relativa es tres caracteres más larga. Es el precio, y es menor que el de tener dos formas distintas de importar en el mismo repositorio.

---

## ADR-037 — El comprimido se expande al encolar, y un archivo que no se abre se queda en su sitio

**Contexto.** Los ZIP y CBZ se expandían dentro del manejador del trabajo (`IPC.sidecarCreateJob`), que es el último sitio por el que pasan las rutas antes de llegar al motor. La cola de la interfaz, en cambio, guardaba la ruta que el usuario soltó: el `.cbz`. El motor recibía rutas de las páginas extraídas, así que `SidecarJobItem::srcPath` apuntaba a `/tmp/superupscaly-archives/<hash>/pagina.jpg` mientras la interfaz buscaba un item cuya ruta fuera el `.cbz`. No coincidía ninguno.

El resultado era exactamente el fallo que el proyecto existe para eliminar: **la lista se quedaba en «pendiente» con la barra a cero, el resumen final decía «N completadas», y las páginas procesadas no aparecían en ninguna parte**. Tampoco había comparación antes y después, porque el `outPath` que autoriza el visor de imágenes viaja en el mismo evento que no encontraba a quién referirse. Nada fallaba con un error: simplemente la interfaz no contaba lo que había pasado.

**Decisión.** La expansión ocurre **al encolar**, no al crear el trabajo. `ingestPaths` valida las rutas, pide al proceso principal que sustituya cada comprimido por sus páginas (`IPC.archivesExpand`) y construye la cola con las rutas reales. Un item de la cola es siempre **una imagen**, y el emparejamiento por ruta de origen vuelve a funcionar sin tocarlo. La expansión del manejador de trabajos se mantiene como red de seguridad: es idempotente (las imágenes pasan tal cual) y sigue siendo quien rechaza el lote con el motivo escrito si algo no se pudo abrir.

Dos consecuencias del mismo cambio, en la misma dirección:

- **Un comprimido que no se puede abrir se queda en la lista, en su sitio**, además de aparecer en `failed`. Antes desaparecía de la salida, así que la lista podía encogerse bajo los dedos del usuario sin decir cuál había sobrado. Ahora hay aviso al añadirlo —con el archivo, el motivo y el código de la taxonomía— y la fila sigue ahí hasta que el usuario decida quitarla.
- **El límite de `statFiles` deja de ser el de la tanda.** Una carpeta se encola como una sola ruta y se expande en el proceso principal, así que la lista de tamaños puede ser mucho mayor que lo que el usuario soltó: a partir de 5000 archivos, los demás se quedaban con `0 B` en silencio. El nuevo `MAX_STAT_FILES` (20 000) coincide con `MAX_EXPAND_FILES`, que es quien decide cuántas rutas puede producir una expansión.

**Consecuencias**

- (+) Cada página tiene su propia fila, su progreso, su resultado y su comparación: el resumen final cuadra con la lista, y el fallo de una página se cuenta en la página.
- (+) La lista de autorización de imágenes del esquema `app://` no se debilita: la llamada nueva **no** autoriza lo que el renderer le pase, solo las páginas que el proceso principal acaba de extraer en su propio directorio temporal (ADR-035 sigue en pie tal cual).
- (+) La interfaz deja de poder contar mal sin que se note: si un CBZ de 200 páginas entra, hay 200 filas que exigen 200 eventos.
- (−) Un CBZ grande llena la lista de filas (una por página) en lugar de una sola. Es lo que realmente se procesa; una fila por archivo comprimido habría sido una media verdad.
- (−) La expansión se intenta dos veces cuando el archivo está roto (al encolar y al crear el trabajo). Es trabajo despreciable —no se extrae nada— y a cambio el aviso llega antes de pulsar el botón.

---

## ADR-038 — La ventana de desarrollo se carga por `localhost`, porque con una IP la página no hidrata

**Contexto.** El usuario abría la aplicación con `npm run dev` y veía la interfaz
entera, pero **no respondía absolutamente a nada**: ni un clic, ni el estado del
motor (siempre «Detenido»), ni la carpeta de salida (siempre «Cargando…»). El
registro no tenía un solo error: ventana cargada, sidecar listo, WebSocket de
eventos conectado.

Medido en la ventana real por CDP: el puente del preload estaba presente (31
métodos), la página se pintaba a 167 fotogramas por segundo, el hilo principal
respondía en 1,5 ms… y **no había ni un solo manejador**: los botones no reaccionaban
ni a eventos de entrada de verdad, y las llamadas al proceso principal nunca se
llegaban a hacer. La causa no era la aplicación: era que la página **no hidrataba**.

El cliente de desarrollo de Next abre su socket de recarga contra el mismo origen
desde el que se sirvió la página. Ese servidor valida el `Host` de la negociación y
la rechaza cuando es una IP. Comprobado a mano contra el mismo servidor:

| `Host` | Respuesta al `upgrade` |
|---|---|
| `127.0.0.1:3456` | ninguna (sin `101`) |
| `localhost:3456` | `101 Switching Protocols` y mensajes de HMR |

Y `scripts/dev.mjs` cargaba la ventana en `http://127.0.0.1:<puerto>` a fuego, así
que en desarrollo la interfaz **nunca** hidrataba. La aplicación empaquetada nunca
estuvo afectada: sirve el export estático por el esquema `app://`, sin cliente de
desarrollo.

**Decisión.** Dos cambios, uno para arreglarlo y otro para que no vuelva por la
puerta de atrás:

1. `scripts/dev.mjs` carga la ventana por `localhost` (y sigue sondeando la salud en
   la IP, que es la dirección que existe siempre). Si el nombre no responde, **lo
   dice** y cae a la IP, en vez de dejar la aplicación sorda en silencio.
2. `next.config.mjs` declara `allowedDevOrigins: ['127.0.0.1', 'localhost']`, para
   que abrir la IP a mano —o un `SU_DEV_URL` con IP— tampoco rompa la hidratación.

**Consecuencias**

- (+) La interfaz responde: verificado en la ventana real, con hidratación, IPC
  contestando (`SuperUpscaly 0.1.0`), estado del motor «Listo» y un clic de entrada
  de verdad abriendo el gestor de modelos.
- (+) El síntoma queda documentado en `docs/06` junto a su comprobación de un
  minuto: abrir `http://localhost:3456/` en un navegador. Un fallo que se manifiesta
  como «no responde nada» y no escribe ni una línea es el peor de diagnosticar; el
  coste de dejarlo escrito es una sección.
- (−) El arranque de desarrollo depende de que `localhost` resuelva. Se acepta
  porque es la dirección que el propio servidor anuncia (`- Local:
  http://localhost:3456`) y porque el caso contrario se avisa en lugar de
degradarse en silencio.
- (−) `allowedDevOrigins` es una opción de desarrollo que no afecta al export
  estático ni al paquete; si Next la retira, el punto 1 sigue bastando.

---

## Resumen de decisiones

| ADR | Decisión | Estado |
|---|---|---|
| 001 | Backend en Rust | Aceptada |
| 002 | Solo ONNX Runtime; NCNN opcional no empaquetado | Aceptada |
| 003 | HTTP loopback + WebSocket (sin gRPC) | Aceptada |
| 004 | Next.js export estático | Aceptada |
| 005 | Jotai con cinco dominios de átomos | Aceptada |
| 006 | Pipelines declarativos en JSON con AST restringido | Aceptada |
| 007 | Tiling adaptativo con calibración + degradación progresiva | Aceptada |
| 008 | 2x/8x derivados de modelos 4x | Aceptada (corregida: el 8x necesita una reducción intermedia) |
| 009 | Cola persistente en SQLite | Aceptada |
| 010 | Telemetría opt-in, local-first | Aceptada |
| 011 | Licencia MIT (modelos aparte) | Aceptada |
| 012 | Instalador base + Acceleration Packs | Aceptada |
| 013 | EXIF aplicado antes de inferir | Aceptada |
| 014 | 1 worker de inferencia por GPU | Aceptada |
| 015 | Escritura atómica + validación de salida | Aceptada |
| 016 | npm workspaces en lugar de pnpm | Aceptada |
| 017 | Sin zod ni i18next en la Fase 1 | Aceptada |
| 018 | ORT detrás de feature, con abstracción `Backend` | Aceptada |
| 019 | La descarga de modelos vive en la aplicación, no en el sidecar | Aceptada |
| 020 | El proveedor de backends es la única fuente de verdad sobre el backend | Aceptada |
| 021 | Cuatro ajustes de la interfaz que el motor ignoraba (`device`, `concurrency`, `unload`, `priority`) | Aceptada |
| 022 | Primera ejecución de las pruebas: 7 defectos de producción y 4 pruebas equivocadas | Aceptada |
| 023 | CSP con hashes de los scripts en línea, y registro de los errores del renderer | Aceptada |
| 024 | Rutas de axum 0.7, `openPath` con límite, y quitar de la UI lo que decía «simulado» | Aceptada |
| 025 | Motor clásico en `f32` en lugar de vecino más cercano; `fallbackModel`, `kernel` y el runtime de ONNX, por fin en marcha | Aceptada |
| 026 | El modelo se descarga en el primer uso; sin él, el motor interpola y lo dice en lugar de perder la imagen | Aceptada |
| 027 | Los píxeles totalmente transparentes toman el color del visible más cercano antes de inferir | Aceptada |
| 028 | La cadena del panel avanzado se proyecta desde los pipelines del motor, y el catálogo se pregunta al arrancar | Aceptada |
| 029 | La silueta (el canal alfa) se escala con el mismo modelo y la misma cadena que el color | Aceptada |
| 030 | Las pistas `tiling` del manifiesto llegan al runner; sin presupuesto, el tile es el mayor que quepa | Aceptada |
| 031 | Peso de etapa según el análisis (`blendFrom`) y umbral del enfoque en la escala real | Aceptada |
| 032 | `onlyOnFaces` recorta cada cara, la restaura y la pega con máscara | Aceptada (falta el detector en el análisis) |
| 033 | Sin copias innecesarias de la imagen; la uniformidad se mide sobre luma Rec. 709 | Aceptada |
| 034 | Pausa, cancelación y reanudación en una sola operación; registro idempotente | Aceptada |
| 035 | Las imágenes del disco se sirven por el esquema propio, con lista de autorización | Aceptada |
| 036 | Extensiones explícitas en las importaciones del paquete compartido | Aceptada |
| 037 | El comprimido se expande al encolar, y el que no se abre se queda en su sitio | Aceptada |
| 038 | La ventana de desarrollo se carga por `localhost` (con una IP no hidrata) | Aceptada |
