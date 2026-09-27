# SuperUpscaly

Aplicación de escritorio para **upscaling de imágenes** con aceleración por hardware (TensorRT / CUDA / DirectML / CoreML / CPU), inspirada en la UX de [Upscayl](https://github.com/upscayl/upscayl) pero con una arquitectura reimplementada desde cero, orientada a:

- **Máxima calidad** de escalado (model chaining: denoise → upscale → face restore → sharpen).
- **Máximo rendimiento** (ONNX Runtime + TensorRT, tiling adaptativo, pipeline asíncrono).
- **Estabilidad real** (sin desbordamientos de VRAM, sin fallos silenciosos, reanudación de lotes).
- **Simplicidad de uso**: una ventana, dos modos, un botón.

> **Estado:** las fases 0 a 4 están entregadas y verificadas; falta la 5 (calidad medida contra Upscayl, empaquetado) y la 6 (QA). La inferencia es **real** con ONNX Runtime: el modelo que necesita el modo elegido se descarga al primer uso y el resultado tiene la calidad de la referencia del usuario ([ver plan](docs/01-plan-de-proyecto.md#9-proximos-pasos-inmediatos)).

---

## Documentación

| Documento | Contenido |
|---|---|
| [01 · Plan de proyecto](docs/01-plan-de-proyecto.md) | Alcance, fases, entregables, criterios de aceptación medibles, riesgos |
| [02 · Arquitectura](docs/02-arquitectura.md) | Componentes, protocolos, pipeline de inferencia, tiling, cola de trabajos, estructura de carpetas, esquemas |
| [03 · Decisiones técnicas (ADR)](docs/03-decisiones-adr.md) | Por qué Rust, por qué ONNX Runtime, por qué HTTP+WS, etc. |
| [04 · Modelos y pipelines](docs/04-modelos-y-pipelines.md) | Catálogo de modelos, cadenas por modo, estrategia 2x/4x/8x, cómo añadir modelos |
| [05 · Guía de usuario](docs/05-guia-de-usuario.md) | Cómo usarlo y **cuándo conviene cambiar cada ajuste** |
| [06 · Solución de problemas](docs/06-solucion-de-problemas.md) | Ordenada por síntoma: el motor no arranca, errores de compilación, errores al procesar |
| [Guía de contribución](CONTRIBUTING.md) | Convenciones y **reglas no negociables** del proyecto |

---

## Resumen de la arquitectura

```
┌──────────────────────────────────────────────────────────────┐
│  Electron (proceso principal)                                │
│   · ventanas, diálogos, validación de rutas                  │
│   · supervisor del sidecar (spawn / health / backoff / kill) │
│   · puente IPC ↔ HTTP/WS                                     │
└───────────────┬──────────────────────────────────────────────┘
                │ contextBridge (preload, sandbox)
┌───────────────▼──────────────────────────────────────────────┐
│  Renderer: Next.js (static export) + React + Tailwind + Jotai│
│   UI morada, drag & drop, selector Fotos / Dibujo-Anime      │
└──────────────────────────────────────────────────────────────┘
                │ HTTP 127.0.0.1:<efímero> + Bearer token
                │ WebSocket para progreso
┌───────────────▼──────────────────────────────────────────────┐
│  Sidecar `su-server` (Rust + axum + ONNX Runtime)            │
│   · cola de trabajos persistente (SQLite)                    │
│   · tiling adaptativo + gestión de VRAM                      │
│   · motor de pipelines (DAG lineal)                          │
│   · gestor de modelos (manifiesto, descarga, verificación)   │
│   · análisis previo (rostros, ruido, tipo de contenido)      │
└──────────────────────────────────────────────────────────────┘
```

**Decisión clave:** el sidecar **lee y escribe los archivos directamente**. Por IPC solo viajan rutas y eventos de progreso, nunca píxeles. Esto elimina el cuello de botella de serialización que limita a otras herramientas.

**Comparar antes y después, sin salir de la aplicación.** La imagen original y el resultado se sirven a la ventana por el propio esquema (`app://superupscaly/media?p=…`), que ya cubre `'self'` en la CSP, y solo para las rutas que el proceso principal autorizó antes: la ventana nunca lee del disco por su cuenta ([ADR-035](docs/03-decisiones-adr.md)).

---

## Stack

| Capa | Tecnología |
|---|---|
| UI | Electron + Next.js (App Router, `output: 'export'`) + TypeScript + React + TailwindCSS + Jotai |
| Backend de inferencia | **Rust** + `axum`/`tokio` + **ONNX Runtime** (crate `ort`, carga dinámica de EP) |
| Procesamiento de imagen | crate `image` + `fast_image_resize` + `libvips` opcional; Sharp solo si se necesita desde Node |
| Persistencia | SQLite (`rusqlite`) para trabajos; JSON para ajustes |
| Monorepo | **npm workspaces** (ver [ADR-016](docs/03-decisiones-adr.md#adr-016--npm-workspaces-en-lugar-de-pnpm)) |
| Empaquetado del main/preload | esbuild |
| Empaquetado de la app | electron-builder (NSIS / DMG / AppImage + DEB) |
| Tests | `cargo test`, contract tests OpenAPI↔TypeScript, Playwright + Electron |

### Estado de implementación

| Fase | Estado |
|---|---|
| 0 · Plan y arquitectura | ✅ completada |
| 1 · Monorepo + UI estática | ✅ completada |
| 2 · Sidecar de inferencia (MVP) | ✅ entregado — cola persistente, API HTTP+WS, CLI |
| 3 · Tiling, VRAM y pipelines | ✅ entregado — composición sin costuras, degradación progresiva |
| 4 · Integración UI ↔ backend | ✅ entregado — inferencia real con ONNX Runtime verificada de punta a punta (ADR-025); la restauración facial funciona por recortes, pero **falta el detector** que alimenta las cajas (ADR-032) |
| 5 · Calidad, rendimiento y empaquetado | pendiente — benchmark contra Upscayl, instaladores |
| 6 · QA y estabilidad | pendiente |

La verificación al día: `cargo test --workspace --all-features` (363 pruebas; 344
sin la feature `onnx`), `npm run typecheck` en los cuatro paquetes y `npm test`
(92 pruebas). Cada ronda de verificación real ha ido encontrando defectos de
producción; están corregidos y documentados en
[ADR-022](docs/03-decisiones-adr.md), [ADR-025](docs/03-decisiones-adr.md),
[ADR-026](docs/03-decisiones-adr.md) y del
[ADR-030](docs/03-decisiones-adr.md) al [ADR-037](docs/03-decisiones-adr.md).

**Motor de escalado.** Con ONNX Runtime y un modelo instalado, la inferencia es
real: medido con el modelo de anime de 6 bloques, 128 px → 512 px en 2,9 s en CPU
y una calidad equivalente a la de la referencia del usuario.

**Los modelos se descargan al primer uso.** Al pulsar «Upscaly», la aplicación
comprueba qué modelo necesita el pipeline del modo y la escala elegidos y, si falta,
lo descarga antes de aceptar el trabajo (18 MB el de anime, con el hash del
manifiesto verificado). La barra de progreso lo dice mientras dura. Sin conexión, o
si un modelo no es descargable, la imagen **no se pierde**: se interpola, el item
queda marcado como degradado y la nota dice con qué motor se hizo.

Sin runtime o sin modelos, el motor cae a **interpolación clásica Catmull-Rom en
`f32`** y lo dice tanto en el registro como en la barra lateral de la interfaz. Ese
respaldo no lava los colores planos, pero tampoco inventa detalle: la diferencia
está medida y explicada en ADR-025. Sobre una ilustración de color plano es la
diferencia entre un borde de 2 px y uno de 6 px.

## Tamaño del proyecto

Deliberadamente contenido. Un upscaler de escritorio no necesita más: la
complejidad está en el tiling, la composición sin costuras y la gestión de VRAM,
no en la cantidad de código.

| Parte | Líneas |
|---|---|
| Sidecar en Rust (11 crates, 363 tests) | ~11.500 |
| Interfaz en TypeScript/TSX | ~4.550 |
| Documentación | ~1.900 |

Más de la mitad del Rust son **tests**: `su-tiling` y `su-core` dedican la mayor
parte de su código a verificar la aritmética de tiles, la normalización de pesos y
la evaluación de condiciones, porque es ahí donde se cometen los errores que no se
ven a simple vista.

Los crates se mantienen pequeños a propósito. Si uno crece, es señal de que
está haciendo dos cosas.

## Plataformas

**Linux es el objetivo principal** del desarrollo. Windows 10/11 y macOS 12+ están soportados por diseño y comparten todo el código; las diferencias reales están acotadas y documentadas.

| Plataforma | Estado | Notas específicas |
|---|---|---|
| **Linux** (AppImage / DEB) | **Objetivo principal** | Drag & drop bajo Wayland requiere portal XDG; si la ruta llega vacía, la app cae al selector nativo. En AppImage, `chrome-sandbox` necesita SUID (lo resuelve el `postinst` del DEB). |
| Windows 10/11 | Soportado | Rutas > 240 caracteres se normalizan con el prefijo `\\?\`. |
| macOS 12+ | Soportado | `titleBarStyle: hiddenInset`, firma y notarización del binario del sidecar. |

Todo el código evita APIs específicas de plataforma salvo en tres puntos, marcados con comentarios: la normalización de rutas largas de Windows, la barra de título de macOS y la selección de binario del sidecar.

## Cómo ejecutarlo

```bash
npm install          # instala todo el monorepo
npm run dev          # dev server de Next + Electron con recarga
npm run build        # export estatico del renderer + bundle del proceso principal
npm start            # ejecuta la app construida
npm run typecheck    # comprobacion de tipos en los cuatro paquetes
```

En la Fase 1 el botón **Upscaly** no escala nada todavía: recorre las mismas
etapas que recorrerá el sidecar con un progreso simulado, para poder validar
estados, cancelación, pausa y resumen antes de que exista la inferencia.

### Sidecar de inferencia (Rust)

```bash
cd services/inference
cargo test          # tests unitarios de todo el workspace
cargo run -p su-cli -- capabilities
cargo run -p su-cli -- upscale foto.png --output ./salida --scale 4
```

Requiere Rust estable (probado con 1.98). **No hace falta ONNX Runtime** para
compilar ni para ejecutar los tests: el escalado funciona con un backend de
referencia. Para usar modelos reales:

```bash
cargo build --release -p su-cli --features onnx
```

## Empaquetado

```bash
npm run package              # instaladores de la plataforma actual
```

Compila el sidecar, lo coloca donde electron-builder lo espera, compila el
renderer y el proceso principal, y genera los instaladores en `release/`.

| Plataforma | Formatos |
|---|---|
| Linux | AppImage + DEB (el `postinst` aplica SUID a `chrome-sandbox`) |
| Windows | NSIS + portable |
| macOS | DMG + ZIP, con *hardened runtime* y entitlements para el sidecar |

El instalador base **no incluye CUDA ni TensorRT**: el sidecar usa `load-dynamic`,
así que el binario de ONNX Runtime se carga en tiempo de ejecución. Eso mantiene la
descarga pequeña para quien no tiene GPU NVIDIA; los aceleradores se ofrecen aparte
como *Acceleration Packs* ([ADR-012](docs/03-decisiones-adr.md)).

## Licencia

MIT para el código. **Los modelos no**: se descargan bajo la licencia de su autor,
que se muestra antes de cada descarga. Ver [LICENSE](LICENSE) para la tabla de
licencias y las restricciones de uso comercial.
